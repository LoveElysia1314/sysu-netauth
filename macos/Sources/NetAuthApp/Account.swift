import AppKit
import NetAuthCore

extension AppDelegate {
    var accountIsModified: Bool {
        guard let saved = savedAccount else { return !username.stringValue.isEmpty || !password.stringValue.isEmpty || manual.state == .on }
        let id = detected.indices.contains(interface.indexOfSelectedItem) ? detected[interface.indexOfSelectedItem].id : ""
        return username.stringValue != saved.username || password.stringValue != saved.password
            || (manual.state == .on) != (saved.iface_mode == "manual")
            || (manual.state == .on && id != saved.iface)
    }
    func updateAuthenticationButton() {
        authenticationButton?.title = currentState == "authenticated" && !accountIsModified ? "重新认证" : "认证"
        authenticationButton?.isEnabled = !busy && !authenticationPreferenceInFlight && !updatePreferencesInFlight && currentState != "authenticating"
    }
    func controlTextDidChange(_ notification: Notification) { accountChanged() }
    @objc func accountChanged() { updateAuthenticationButton() }
    func confirmDiscardAccount() -> Bool {
        guard !busy, !authenticationPreferenceInFlight, !updatePreferencesInFlight else { footer.stringValue = "正在应用设置，请稍候。"; return false }
        guard accountIsModified else { return true }
        let alert = NSAlert()
        alert.messageText = "账户修改尚未应用"
        alert.informativeText = "点击“认证”才会保存并使用当前账户。是否放弃此次修改？"
        alert.addButton(withTitle: "继续编辑")
        alert.addButton(withTitle: "放弃修改")
        guard alert.runModal() == .alertSecondButtonReturn else { return false }
        username.stringValue = savedAccount?.username ?? ""
        password.stringValue = savedAccount?.password ?? ""
        manual.state = savedAccount?.iface_mode == "manual" ? .on : .off
        if let saved = savedAccount, let index = detected.firstIndex(where: { $0.id == saved.iface }) { interface.selectItem(at: index) }
        selectMode()
        return true
    }
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        uninstallCompleted || notificationsOnly || confirmDiscardAccount() ? .terminateNow : .terminateCancel
    }
    @objc func changeAutoAuthentication() {
        guard !busy, demo || preferencesLoaded else { autoAuth.state = savedAutoAuth ? .on : .off; return }
        if demo { footer.stringValue = "离线模拟：未保存设置。"; return }
        let enabled = autoAuth.state == .on
        authenticationPreferenceInFlight = true
        updateAuthenticationButton()
        autoAuth.isEnabled = false
        DispatchQueue.global(qos: .utility).async {
            let result = Result { try self.client.saveAuthenticationPreference(enabled, authorized: true) }
            DispatchQueue.main.async {
                self.authenticationPreferenceInFlight = false
                self.autoAuth.isEnabled = !self.busy
                self.updateAuthenticationButton()
                switch result {
                case .success: self.savedAutoAuth = enabled; self.footer.stringValue = "自动认证设置已保存。"
                case .failure(let error): self.autoAuth.state = self.savedAutoAuth ? .on : .off; self.footer.stringValue = error.localizedDescription
                }
            }
        }
    }
}
