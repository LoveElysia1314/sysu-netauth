import AppKit
import Carbon
import ServiceManagement
import UserNotifications
import NetAuthCore
import NetAuthArtwork

extension AppDelegate: NSWindowDelegate, UNUserNotificationCenterDelegate, NSTextFieldDelegate {
    func configureDesktop() {
        showWindowObserver = DistributedNotificationCenter.default().addObserver(forName: .init("io.github.peterrobot.sysu-netauth.show-window"), object: String(getuid()), queue: .main) { [weak self] _ in self?.showWindow() }
        window.delegate = self
        window.isReleasedWhenClosed = false
        UNUserNotificationCenter.current().delegate = self
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        statusItem = item
        let menu = NSMenu()
        menu.autoenablesItems = false
        menuStatus = NSMenuItem(title: "SYSU NetAuth · 正在读取", action: nil, keyEquivalent: "")
        menuStatus?.isEnabled = false
        menu.addItem(menuStatus!)
        menu.addItem(.separator())
        for (title, selector) in [("显示主窗口", #selector(showWindow)), ("刷新状态", #selector(refresh)),
                                  ("认证", #selector(connect)), ("断开连接", #selector(disconnect))] {
            if selector == #selector(refresh) {
                if #available(macOS 13, *) { continue }
            }
            let entry = NSMenuItem(title: title, action: selector, keyEquivalent: "")
            entry.target = self
            menu.addItem(entry)
            if selector != #selector(showWindow) { menuActions.append(entry) }
        }
        menu.addItem(makeIconMenu())
        menu.addItem(makeUpdateMenu())
        menu.addItem(.separator())
        let quit = NSMenuItem(title: "退出 SYSU NetAuth", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        quit.target = NSApp
        menu.addItem(quit)
        item.menu = menu
        updateStatusItem("idle")
        iconAppearanceObserver = item.button?.observe(\.effectiveAppearance, options: [.new]) { [weak self] _, _ in
            guard let self = self, !self.iconRedrawPending else { return }
            self.iconRedrawPending = true
            DispatchQueue.main.async { [weak self] in
                self?.iconRedrawPending = false
                self?.redrawStatusIcon()
            }
        }
        login.target = self
        login.action = #selector(changeLogin)
        hidden.target = self
        hidden.action = #selector(changeHidden)
        notify.target = self
        notify.action = #selector(changeNotifications)
        notifyUpdates.target = self
        notifyUpdates.action = #selector(changeUpdateNotifications)
        autoCheck.target = self
        autoCheck.action = #selector(changeUpdateNotifications)
        notifyUpdates.state = localUpdateNotifications ? .on : .off
        hidden.state = UserDefaults.standard.object(forKey: "hideOnLogin") == nil || UserDefaults.standard.bool(forKey: "hideOnLogin") ? .on : .off
        notify.state = UserDefaults.standard.bool(forKey: "stateNotifications") ? .on : .off
        syncLoginStatus()
        if !demo, let executable = Bundle.main.executableURL { try? NotificationAgentLease.start(executable: executable) }
        if !demo, #available(macOS 13, *) {
            pollTimer = Timer.scheduledTimer(withTimeInterval: 5, repeats: true) { [weak self] _ in
                self?.pollStatus()
            }
            pollTimer?.tolerance = 1
        }
    }
    func pollStatus() {
        guard !busy, !pollInFlight else { return }
        if !notificationsOnly && !preferencesLoaded {
            runAction(.status, authorized: true)
            return
        }
        if notificationsOnly { notify.state = UserDefaults.standard.bool(forKey: "stateNotifications") ? .on : .off }
        pollInFlight = true
        let generation = statusGeneration
        DispatchQueue.global(qos: .utility).async {
            let result = Result { try JSONDecoder().decode(Snapshot.self, from: self.client.run(.status, authorized: true)) }
            DispatchQueue.main.async {
                self.pollInFlight = false
                guard !self.busy, self.statusGeneration == generation else { return }
                switch result {
                case .success(let snapshot): self.applySnapshot(snapshot)
                case .failure(let error):
                    self.connectionRequest = nil
                    self.noticeTimer?.invalidate()
                    self.updateStatusItem("unavailable")
                    self.status.stringValue = "服务未连接"
                    self.status.textColor = .systemOrange
                    self.details.isHidden = false
                    self.details.stringValue = String(error.localizedDescription.prefix(180))
                }
            }
        }
    }
    func applySnapshot(_ snapshot: Snapshot) {
        currentState = snapshot.state
        updateAuthenticationButton()
        updateNotificationsEnabled = snapshot.update_notify ?? true
        applyUpdate(snapshot.update)
        if notificationsOnly { observeDesktop(snapshot); return }
        refreshAdapters(snapshot.adapters)
        observeDesktop(snapshot)
        status.stringValue = DesktopState.label(snapshot.state)
        status.textColor = snapshot.state == "authenticated" ? .systemGreen : .labelColor
        details.isHidden = true
        details.stringValue = ""
        status.toolTip = snapshot.message
        showNetwork(snapshot.adapter)
    }
    func refreshAdapters(_ all: [Adapter]) {
        let selected = manual.state == .on && detected.indices.contains(interface.indexOfSelectedItem)
            ? detected[interface.indexOfSelectedItem] : nil
        let items = AdapterSelection.refresh(all, selected: selected)
        guard items != detected else { return }
        detected = items
        interface.removeAllItems()
        interface.addItems(withTitles: items.isEmpty ? ["未发现有线网卡"] : items.map { $0.up ? $0.name : "\($0.name)（未连接）" })
        if let selected = selected, let index = items.firstIndex(where: { $0.id == selected.id }) {
            interface.selectItem(at: index)
        }
        selectMode()
    }
    func syncLoginStatus() {
        if #available(macOS 13, *) {
            let service = SMAppService.mainApp
            login.state = service.status == .enabled || service.status == .requiresApproval ? .on : .off
            hidden.isEnabled = login.state == .on
            if service.status == .requiresApproval {
                footer.stringValue = "登录项需要批准，请在系统设置 → 通用 → 登录项中允许 SYSU NetAuth。"
            }
        } else {
            login.toolTip = "登录启动需要 macOS 13 或更新版本。"
            login.isEnabled = false
            hidden.isEnabled = false
        }
    }
    @objc func changeLogin() {
        guard #available(macOS 13, *) else { return }
        if demo {
            login.state = .off
            footer.stringValue = "离线模拟：未更改系统登录项。"
            return
        }
        do {
            if login.state == .on { try SMAppService.mainApp.register() }
            else { try SMAppService.mainApp.unregister() }
            footer.stringValue = "登录启动设置已更新。请保持应用所在路径不变。"
        } catch { footer.stringValue = "登录项更新失败：\(error.localizedDescription)" }
        syncLoginStatus()
    }
    @objc func changeHidden() { UserDefaults.standard.set(hidden.state == .on, forKey: "hideOnLogin") }
    @objc func changeNotifications() {
        if notify.state == .off {
            noticeTimer?.invalidate()
            UserDefaults.standard.set(false, forKey: "stateNotifications")
            UNUserNotificationCenter.current().removePendingNotificationRequests(withIdentifiers: ["sysu-netauth-state"])
            return
        }
        notify.state = .off
        footer.stringValue = "正在请求通知权限；若未出现提示，请在系统设置 → 通知中允许 SYSU NetAuth。"
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { granted, error in
            DispatchQueue.main.async {
                self.notify.state = granted ? .on : .off
                UserDefaults.standard.set(granted, forKey: "stateNotifications")
                self.footer.stringValue = granted ? "状态通知已启用。" : "通知未获授权，请在系统设置 → 通知中允许 SYSU NetAuth。"
                if let error = error { self.footer.stringValue = "通知授权失败：\(error.localizedDescription)" }
            }
        }
    }
    func postNotification(title: String, body: String) {
        guard notify.state == .on, ownsNotifications else { return }
        let center = UNUserNotificationCenter.current()
        center.getNotificationSettings { settings in
            guard settings.authorizationStatus == .authorized || settings.authorizationStatus == .provisional else { return }
            let content = UNMutableNotificationContent()
            content.title = title; content.body = body; content.sound = .default
            center.add(UNNotificationRequest(identifier: "sysu-netauth-state", content: content, trigger: nil)) { error in
                if let error = error { DispatchQueue.main.async { self.footer.stringValue = "通知发送失败：\(error.localizedDescription)" } }
            }
        }
    }
    func observeDesktop(_ snapshot: Snapshot) {
        if let current = snapshot.connection_request {
            if let previous = connectionRequest, previous != current, current != 0 { notices.serviceConnectRequested(current: snapshot.state) }
            connectionRequest = current
        }
        if !notificationsOnly { updateStatusItem(snapshot.state, message: snapshot.message) }
        let now = ProcessInfo.processInfo.systemUptime
        if let notice = notices.observe(snapshot.state, paused: snapshot.suspended || snapshot.manual_disconnect,
                                        enabled: notify.state == .on && ownsNotifications, now: now) {
            postNotification(title: "SYSU NetAuth", body: notice.message(state: snapshot.state))
        }
        noticeTimer?.invalidate()
        if notify.state == .on && ownsNotifications, let deadline = notices.deadline {
            noticeTimer = Timer.scheduledTimer(withTimeInterval: max(0.01, deadline - now), repeats: false) { [weak self] _ in
                guard let self = self else { return }
                if let notice = self.notices.poll(enabled: self.notify.state == .on, now: ProcessInfo.processInfo.systemUptime) {
                    self.postNotification(title: "SYSU NetAuth", body: notice.message(state: snapshot.state))
                }
            }
        }
    }
    func makeIconMenu() -> NSMenuItem {
        let parent = NSMenuItem(title: "菜单栏图标", action: nil, keyEquivalent: "")
        let menu = NSMenu()
        menu.autoenablesItems = false
        for (tag, label) in [(0, "单色 · 跟随系统"), (1, "彩色 · 共享状态配色")] {
            let item = NSMenuItem(title: label, action: #selector(changeIconMode(_:)), keyEquivalent: "")
            item.tag = tag
            item.target = self
            menu.addItem(item)
            iconModeItems.append(item)
        }
        parent.submenu = menu
        syncIconMode()
        return parent
    }
    func syncIconMode() {
        let selected = UserDefaults.standard.bool(forKey: "coloredStatusIcon") ? 1 : 0
        for item in iconModeItems { item.state = item.tag == selected ? .on : .off }
    }
    @objc func changeIconMode(_ sender: NSMenuItem) {
        UserDefaults.standard.set(sender.tag == 1, forKey: "coloredStatusIcon")
        syncIconMode()
        redrawStatusIcon()
    }
    func redrawStatusIcon() {
        guard let button = statusItem?.button else { return }
        let colored = UserDefaults.standard.bool(forKey: "coloredStatusIcon")
        let appearance = button.effectiveAppearance
        let name = appearance.bestMatch(from: [.aqua, .darkAqua,
            .accessibilityHighContrastAqua, .accessibilityHighContrastDarkAqua]) ?? appearance.name
        let key = StatusIconRenderKey(mark: currentMark, colored: colored, appearance: name.rawValue)
        guard iconRenderKey != key else { return }
        // Assign before changing the button: AppKit can notify effectiveAppearance again
        // during drawing. Replacing its images on every notification creates a redraw loop.
        iconRenderKey = key
        appearance.performAsCurrentDrawingAppearance {
            button.image = StatusIcon.image(currentMark, colored: colored)
            button.alternateImage = StatusIcon.image(currentMark, colored: colored, highlighted: true)
        }
    }
    func updateStatusItem(_ state: String, message: String = "") {
        currentMark = StatusMark.resolve(state: state, message: message)
        redrawStatusIcon()
        let label = currentMark.label
        statusItem?.button?.toolTip = "SYSU NetAuth · \(label)"
        statusItem?.button?.setAccessibilityLabel("SYSU NetAuth · \(label)")
        menuStatus?.title = "SYSU NetAuth · \(label)"
    }
    @objc func showWindow() {
        if notificationsOnly {
            if let executable = Bundle.main.executableURL { try? NotificationAgentLease.launch(executable: executable) }
            return
        }
        NSApp.setActivationPolicy(.regular)
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }
    @objc func closeMainWindow() { _ = windowShouldClose(window) }
    func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard confirmDiscardAccount() else { return false }
        sender.orderOut(nil)
        NSApp.setActivationPolicy(.accessory)
        return false
    }
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool { showWindow(); return true }
    func applicationDidBecomeActive(_ notification: Notification) {
        guard window != nil else { return }
        syncLoginStatus()
        if UserDefaults.standard.bool(forKey: "stateNotifications") {
            UNUserNotificationCenter.current().getNotificationSettings { settings in
                DispatchQueue.main.async {
                    self.notify.state = settings.authorizationStatus == .authorized || settings.authorizationStatus == .provisional ? .on : .off
                }
            }
        }
    }
    func applicationWillTerminate(_ notification: Notification) {
        pollTimer?.invalidate(); noticeTimer?.invalidate(); iconAppearanceObserver?.invalidate()
        if let observer = showWindowObserver { DistributedNotificationCenter.default().removeObserver(observer) }
    }
    func shouldStartHidden() -> Bool {
        let event = NSAppleEventManager.shared().currentAppleEvent
        let loginEvent = event?.paramDescriptor(forKeyword: AEKeyword(keyAEPropData))?
            .paramDescriptor(forKeyword: AEKeyword(keyAELaunchedAsLogInItem))?.booleanValue ?? false
        return DesktopState.hideAtLaunch(loginLaunch: loginEvent, hideEnabled: hidden.state == .on)
    }
    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification,
                                withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
        completionHandler([.banner, .list, .sound])
    }
    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
                                withCompletionHandler completionHandler: @escaping () -> Void) {
        DispatchQueue.main.async {
            if let value = response.notification.request.content.userInfo["releaseURL"] as? String,
               let url = ReleaseUpdate.trustedURL(value) { NSWorkspace.shared.open(url) }
            else { self.showWindow() }
            completionHandler()
        }
    }
}
