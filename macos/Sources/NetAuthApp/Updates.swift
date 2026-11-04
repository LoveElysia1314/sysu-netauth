import AppKit
import UserNotifications
import NetAuthCore

extension AppDelegate {
    var ownsNotifications: Bool { notificationsOnly || !NotificationAgentLease.isRunning }
    var localUpdateNotifications: Bool {
        UserDefaults.standard.object(forKey: "updateNotifications") == nil || UserDefaults.standard.bool(forKey: "updateNotifications")
    }
    @objc func changeUpdateNotifications() {
        let preferences = UpdatePreferences(autoCheck: autoCheck.state == .on, notify: notifyUpdates.state == .on)
        if demo { footer.stringValue = "离线模拟：未保存更新设置。"; return }
        guard preferencesLoaded else { return }
        updatePreferencesInFlight = true
        updateAuthenticationButton()
        autoCheck.isEnabled = false
        notifyUpdates.isEnabled = false
        DispatchQueue.global(qos: .utility).async {
            let result = Result { try self.client.saveUpdatePreferences(preferences, authorized: true) }
            DispatchQueue.main.async {
                self.updatePreferencesInFlight = false
                self.updateAuthenticationButton()
                self.autoCheck.isEnabled = !self.busy
                self.notifyUpdates.isEnabled = !self.busy
                switch result {
                case .success:
                    self.savedAutoUpdateCheck = preferences.auto_update_check
                    self.savedUpdateNotify = preferences.update_notify
                    UserDefaults.standard.set(preferences.update_notify, forKey: "updateNotifications")
                    self.attemptedUpdateVersion = ""
                    self.footer.stringValue = "更新设置已保存。"
                    self.pollStatus()
                case .failure(let error):
                    self.autoCheck.state = self.savedAutoUpdateCheck ? .on : .off
                    self.notifyUpdates.state = self.savedUpdateNotify ? .on : .off
                    self.footer.stringValue = "更新设置未保存：\(error.localizedDescription)"
                }
            }
        }
        if notifyUpdates.state == .off {
            UNUserNotificationCenter.current().removePendingNotificationRequests(withIdentifiers: ["sysu-netauth-update"])
        }
    }
    func showUpdateLabel(_ text: String) {
        updateLabel.stringValue = text
        updateInfoItems.forEach { $0.title = text }
    }
    func configureNotificationAgent() {
        guard #available(macOS 13, *) else { NSApp.terminate(nil); return }
        UNUserNotificationCenter.current().delegate = self
        pollTimer = Timer.scheduledTimer(withTimeInterval: 5, repeats: true) { [weak self] _ in self?.pollStatus() }
        pollTimer?.tolerance = 1
        pollStatus()
    }
    func makeUpdateMenu() -> NSMenuItem {
        let parent = NSMenuItem(title: "软件更新", action: nil, keyEquivalent: "")
        let menu = NSMenu()
        menu.autoenablesItems = false
        let info = NSMenuItem(title: "尚未检查更新", action: nil, keyEquivalent: "")
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
        let current = "当前版本 v" + (Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0.7.0")
        if update.checking { label = "正在检查更新…" }
        else if visible { label = "存在新版本：v\(update.latest_version)" }
        else if update.available && update.latest_version == UserDefaults.standard.string(forKey: "ignoredRelease") { label = "已忽略 v\(update.latest_version)" }
        else if !update.error.isEmpty { label = "检查失败，请稍后重试" }
        else { label = update.checked_at == 0 ? current : current + " · 已是最新" }
        showUpdateLabel(label)
        updateCheckButton?.isEnabled = !update.checking && !updateRequestInFlight
        updateReleaseButton?.isHidden = false
        updateReleaseButton?.title = "更新日志"
        updateLabel.isHidden = false
        updateReleaseItems.forEach { $0.isEnabled = visible }
        updateIgnoreItems.forEach { $0.isEnabled = visible }
        guard updateNotificationsEnabled, localUpdateNotifications, ownsNotifications, !demo, attemptedUpdateVersion != update.latest_version,
              update.shouldNotify(ignored: UserDefaults.standard.string(forKey: "ignoredRelease"),
                                  notified: UserDefaults.standard.string(forKey: "notifiedRelease")) else { return }
        attemptedUpdateVersion = update.latest_version
        let center = UNUserNotificationCenter.current()
        let deliver: () -> Void = {
            guard self.localUpdateNotifications else { return }
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
        updateCheckButton?.isEnabled = false
        showUpdateLabel("正在检查更新…")
        DispatchQueue.global(qos: .utility).async {
            let result = Result { try JSONDecoder().decode(ReleaseUpdate.self, from: self.client.run(.checkUpdate, authorized: true)) }
            DispatchQueue.main.async {
                self.updateRequestInFlight = false
                self.updateCheckButton?.isEnabled = true
                switch result {
                case .success(let update): self.applyUpdate(update)
                case .failure: self.showUpdateLabel("检查失败，请稍后重试")
                }
            }
        }
    }
    @objc func openRelease() {
        if let url = releaseUpdate?.url { NSWorkspace.shared.open(url) }
        else { openChangelog() }
    }
    @objc func openChangelog() {
        let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0.7.0"
        let url = releaseUpdate?.changelogURL(currentVersion: version, ignored: UserDefaults.standard.string(forKey: "ignoredRelease"))
            ?? URL(string: "https://github.com/LoveElysia1314/sysu-netauth/releases/tag/v" + version)
        if let url = url { NSWorkspace.shared.open(url) }
    }
    @objc func ignoreRelease() {
        guard let update = releaseUpdate else { return }
        UserDefaults.standard.set(update.latest_version, forKey: "ignoredRelease")
        applyUpdate(update)
    }
}
