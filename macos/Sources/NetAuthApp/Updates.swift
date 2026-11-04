import AppKit
import UserNotifications
import NetAuthCore

extension AppDelegate {
    var ownsNotifications: Bool { notificationsOnly || !NotificationAgentLease.isRunning }
    func configureNotificationAgent() {
        guard #available(macOS 13, *) else { NSApp.terminate(nil); return }
        UNUserNotificationCenter.current().delegate = self
        notificationTestObserver = DistributedNotificationCenter.default().addObserver(
            forName: .init("io.github.peterrobot.sysu-netauth.notification-test"), object: String(getuid()), queue: .main
        ) { [weak self] _ in
            guard let self = self else { return }
            self.notify.state = UserDefaults.standard.bool(forKey: "stateNotifications") ? .on : .off
            self.postNotification(title: "SYSU NetAuth · 离线测试", body: "本通知来自独立通知助手，未更改校园网认证。")
        }
        pollTimer = Timer.scheduledTimer(withTimeInterval: 5, repeats: true) { [weak self] _ in self?.pollStatus() }
        pollTimer?.tolerance = 1
        pollStatus()
    }
    func makeUpdateMenu() -> NSMenuItem {
        let parent = NSMenuItem(title: "软件更新", action: nil, keyEquivalent: "")
        let menu = NSMenu()
        menu.autoenablesItems = false
        let info = NSMenuItem(title: "仅检查正式版更新", action: nil, keyEquivalent: "")
        info.isEnabled = false
        menu.addItem(info)
        updateInfoItems.append(info)
        for (title, selector) in [("检查更新", #selector(checkForUpdates)), ("查看新版说明", #selector(openRelease)), ("忽略此版本", #selector(ignoreRelease))] {
            let item = NSMenuItem(title: title, action: selector, keyEquivalent: "")
            item.target = self
            menu.addItem(item)
            if selector == #selector(openRelease) { item.isEnabled = false; updateReleaseItems.append(item) }
            if selector == #selector(ignoreRelease) { item.isEnabled = false; updateIgnoreItems.append(item) }
        }
        parent.submenu = menu
        return parent
    }
    func applyUpdate(_ update: ReleaseUpdate?) {
        guard let update = update else { return }
        releaseUpdate = update
        let visible = update.available && update.url != nil && update.latest_version != UserDefaults.standard.string(forKey: "ignoredRelease")
        let label: String
        if update.checking { label = "正在检查正式版更新…" }
        else if visible { label = "发现正式版 \(update.latest_version)" }
        else if !update.error.isEmpty { label = "检查失败：\(update.error.prefix(100))" }
        else { label = update.checked_at == 0 ? "仅检查正式版更新" : "暂无更新的正式版" }
        updateInfoItems.forEach { $0.title = label }
        updateReleaseItems.forEach { $0.isEnabled = visible }
        updateIgnoreItems.forEach { $0.isEnabled = visible }
        guard ownsNotifications, !demo, attemptedUpdateVersion != update.latest_version,
              update.shouldNotify(ignored: UserDefaults.standard.string(forKey: "ignoredRelease"),
                                  notified: UserDefaults.standard.string(forKey: "notifiedRelease")) else { return }
        attemptedUpdateVersion = update.latest_version
        let center = UNUserNotificationCenter.current()
        let deliver: () -> Void = {
            guard update.shouldNotify(ignored: UserDefaults.standard.string(forKey: "ignoredRelease"), notified: UserDefaults.standard.string(forKey: "notifiedRelease")) else { return }
            let content = UNMutableNotificationContent()
            content.title = "SYSU NetAuth 更新"
            content.body = "发现正式版 \(update.latest_version)：\(update.summary)"
            content.sound = .default
            content.userInfo = ["releaseURL": update.release_url]
            center.add(UNNotificationRequest(identifier: "sysu-netauth-update", content: content, trigger: nil)) { error in
                if error == nil { UserDefaults.standard.set(update.latest_version, forKey: "notifiedRelease") }
            }
        }
        center.getNotificationSettings { settings in
            switch settings.authorizationStatus {
            case .authorized, .provisional: deliver()
            case .notDetermined:
                center.requestAuthorization(options: [.alert, .sound]) { granted, _ in if granted { deliver() } }
            default: break
            }
        }
    }
    @objc func checkForUpdates() {
        guard !updateRequestInFlight else { return }
        if demo { footer.stringValue = "离线模拟：未访问更新源。"; return }
        updateRequestInFlight = true
        updateInfoItems.forEach { $0.title = "正在检查正式版更新…" }
        DispatchQueue.global(qos: .utility).async {
            let result = Result { try JSONDecoder().decode(ReleaseUpdate.self, from: self.client.run(.checkUpdate, authorized: true)) }
            DispatchQueue.main.async {
                self.updateRequestInFlight = false
                switch result {
                case .success(let update): self.applyUpdate(update)
                case .failure(let error): self.updateInfoItems.forEach { $0.title = "检查失败：\(error.localizedDescription.prefix(100))" }
                }
            }
        }
    }
    @objc func openRelease() { if let url = releaseUpdate?.url { NSWorkspace.shared.open(url) } }
    @objc func ignoreRelease() {
        guard let update = releaseUpdate else { return }
        UserDefaults.standard.set(update.latest_version, forKey: "ignoredRelease")
        applyUpdate(update)
    }
}
