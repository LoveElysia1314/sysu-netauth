import AppKit
import NetAuthCore

extension AppDelegate {
    @objc func viewDiagnostics() { loadDiagnostics(export: false) }
    @objc func exportDiagnostics() { loadDiagnostics(export: true) }
    private func loadDiagnostics(export: Bool) {
        guard !busy else { return }
        if demo { footer.stringValue = "离线模拟：未读取后台日志。"; return }
        setBusy(true)
        DispatchQueue.global(qos: .utility).async {
            let result = Result { try JSONDecoder().decode(DiagnosticReport.self,
                from: self.client.run(.diagnostics, authorized: true)) }
            DispatchQueue.main.async {
                self.setBusy(false)
                switch result {
                case .failure(let error): self.footer.stringValue = error.localizedDescription
                case .success(let report):
                    if export {
                        let panel = NSSavePanel()
                        panel.nameFieldStringValue = "SYSUNetAuth-diagnostics.json"
                        panel.message = "诊断包含最近服务日志；不包含账号配置，当前账号与密码会被遮盖。分享前请自行检查其他个人信息。"
                        guard panel.runModal() == .OK, let destination = panel.url else { return }
                        do {
                            let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
                            try encoder.encode(report).write(to: destination, options: .atomic)
                            try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: destination.path)
                            self.footer.stringValue = "诊断信息已导出。"
                        } catch { self.footer.stringValue = error.localizedDescription }
                    } else {
                        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 680, height: 420),
                            styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
                        window.title = "SYSU NetAuth · 最近服务日志"
                        window.isReleasedWhenClosed = false
                        let scroll = NSScrollView(frame: window.contentView!.bounds)
                        scroll.autoresizingMask = [.width, .height]; scroll.hasVerticalScroller = true
                        let text = NSTextView(frame: scroll.bounds)
                        text.isEditable = false
                        text.font = .monospacedSystemFont(ofSize: 12, weight: .regular)
                        text.string = report.log.isEmpty ? "暂无服务日志。" : report.log
                        text.autoresizingMask = [.width]
                        scroll.documentView = text; window.contentView!.addSubview(scroll)
                        self.diagnosticsWindow?.close(); self.diagnosticsWindow = window
                        window.center(); window.makeKeyAndOrderFront(nil)
                    }
                }
            }
        }
    }
}
