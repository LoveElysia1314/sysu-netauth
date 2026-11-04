import AppKit
import ServiceManagement
import UserNotifications
import NetAuthCore

extension AppDelegate {
    @objc func uninstallApplication() {
        if demo { footer.stringValue = "离线模拟：未卸载任何组件。"; return }
        guard !busy, !authenticationPreferenceInFlight, !updatePreferencesInFlight else { return }
        guard Bundle.main.bundleURL.standardizedFileURL.path == "/Applications/SYSUNetAuth.app",
              let script = Bundle.main.resourceURL?.appendingPathComponent("uninstall-macos.sh"),
              FileManager.default.fileExists(atPath: script.path) else {
            footer.stringValue = "请从“应用程序”打开 SYSU NetAuth 后执行卸载。"; return
        }
        let alert = NSAlert()
        alert.messageText = "卸载 SYSU NetAuth？"
        alert.informativeText = "将停止认证和通知，移除应用、后台服务及授权组件。macOS 将请求管理员授权。"
        alert.addButton(withTitle: "卸载")
        alert.addButton(withTitle: "取消")
        let purge = NSButton(checkboxWithTitle: "同时删除账户、设置和日志", target: nil, action: nil)
        alert.accessoryView = purge
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        let deleteSettings = purge.state == .on
        var restoreLogin = false
        do {
            if #available(macOS 13, *) {
                let service = SMAppService.mainApp
                restoreLogin = service.status == .enabled || service.status == .requiresApproval
                if restoreLogin { try service.unregister() }
            }
        } catch {
            footer.stringValue = "无法移除自动启动项：\(error.localizedDescription)"; return
        }
        setBusy(true)
        pollTimer?.invalidate()
        noticeTimer?.invalidate()
        DispatchQueue.global(qos: .userInitiated).async { [self] in
            var arguments = [script.path, "--remove-app"]
            if deleteSettings { arguments.append("--purge") }
            let result = Result { try Client(executable: URL(fileURLWithPath: "/bin/sh"))
                .execute(arguments: arguments, timeout: 120, authorized: true) }
            DispatchQueue.main.async {
                switch result {
                case .success:
                    UNUserNotificationCenter.current().removeAllPendingNotificationRequests()
                    UNUserNotificationCenter.current().removeAllDeliveredNotifications()
                    if deleteSettings, let identifier = Bundle.main.bundleIdentifier {
                        UserDefaults.standard.removePersistentDomain(forName: identifier)
                    }
                    self.uninstallCompleted = true
                    NSApp.terminate(nil)
                case .failure(let error):
                    if restoreLogin, #available(macOS 13, *) { try? SMAppService.mainApp.register() }
                    self.setBusy(false)
                    self.syncLoginStatus()
                    self.footer.stringValue = "卸载未完成：\(error.localizedDescription)"
                    self.pollTimer = Timer.scheduledTimer(withTimeInterval: 5, repeats: true) { [weak self] _ in self?.pollStatus() }
                    self.pollTimer?.tolerance = 1
                    self.pollStatus()
                }
            }
        }
    }
}
