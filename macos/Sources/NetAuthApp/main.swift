import AppKit
import NetAuthCore

final class AppDelegate: NSObject, NSApplicationDelegate {
    var window: NSWindow!
    let username = NSTextField()
    let password = NSSecureTextField()
    let interface = NSPopUpButton()
    let manual = NSButton(checkboxWithTitle: "手动选择网卡", target: nil, action: nil)
    let autoAuth = NSButton(checkboxWithTitle: "服务启动后自动认证", target: nil, action: nil)
    var retryInterval = 60
    var preferencesLoaded = false
    var diagnosticsWindow: NSWindow?
    let status = NSTextField(labelWithString: "正在读取…")
    let details = NSTextField(wrappingLabelWithString: "")
    let network = NSTextField(wrappingLabelWithString: "")
    let footer = NSTextField(wrappingLabelWithString: "")
    var buttons: [NSButton] = []
    var detected: [Adapter] = []
    var busy = false
    let login = NSButton(checkboxWithTitle: "开机后启动界面程序", target: nil, action: nil)
    let hidden = NSButton(checkboxWithTitle: "开机后隐藏配置窗口", target: nil, action: nil)
    let notify = NSButton(checkboxWithTitle: "认证状态变化后通知", target: nil, action: nil)
    var statusItem: NSStatusItem?
    var menuStatus: NSMenuItem?
    var menuActions: [NSMenuItem] = []
    var pollTimer: Timer?
    var noticeTimer: Timer?
    var pollInFlight = false
    var statusGeneration = 0
    var notices = StateNotifications()
    var currentMark: StatusMark = .ready
    var iconModeItems: [NSMenuItem] = []
    var iconAppearanceObserver: NSKeyValueObservation?
    var iconRenderKey: StatusIconRenderKey?
    var iconRedrawPending = false
    let demo = ProcessInfo.processInfo.environment["NETAUTH_CLI"]?.hasSuffix("mock-cli.py") == true
    let client = Client(executable: Client.resolveExecutable(),
                        dataDirectory: ProcessInfo.processInfo.environment["NETAUTH_DATA_DIR"])

    func column(_ views: [NSView], spacing: CGFloat = 12) -> NSStackView {
        let stack = NSStackView(views: views)
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = spacing
        return stack
    }
    func row(_ label: String, _ control: NSView) -> NSStackView {
        let text = NSTextField(labelWithString: label)
        text.widthAnchor.constraint(equalToConstant: 44).isActive = true
        let stack = NSStackView(views: [text, control])
        stack.spacing = 8
        control.widthAnchor.constraint(equalToConstant: 252).isActive = true
        return stack
    }
    func button(_ title: String, _ action: Selector) -> NSButton {
        let b = NSButton(title: title, target: self, action: action)
        b.bezelStyle = .rounded
        buttons.append(b)
        return b
    }
    func box(_ title: String, _ content: NSView) -> NSBox {
        let box = NSBox()
        box.title = title
        box.contentViewMargins = NSSize(width: 14, height: 14)
        box.contentView = NSView()
        content.translatesAutoresizingMaskIntoConstraints = false
        box.contentView!.addSubview(content)
        NSLayoutConstraint.activate([
            content.leadingAnchor.constraint(equalTo: box.contentView!.leadingAnchor),
            content.trailingAnchor.constraint(equalTo: box.contentView!.trailingAnchor),
            content.topAnchor.constraint(equalTo: box.contentView!.topAnchor),
            content.bottomAnchor.constraint(lessThanOrEqualTo: box.contentView!.bottomAnchor)
        ])
        return box
    }
    func applicationDidFinishLaunching(_ notification: Notification) {
        if let url = Bundle.main.url(forResource: "AppIcon", withExtension: "icns"), let icon = NSImage(contentsOf: url) {
            NSApp.applicationIconImage = icon
        }
        let menu = NSMenu()
        let appItem = NSMenuItem()
        menu.addItem(appItem)
        let appMenu = NSMenu()
        let show = NSMenuItem(title: "显示主窗口", action: #selector(showWindow), keyEquivalent: "")
        show.target = self
        appMenu.addItem(show)
        let close = NSMenuItem(title: "关闭窗口", action: #selector(closeMainWindow), keyEquivalent: "w")
        close.target = self
        appMenu.addItem(close)
        appMenu.addItem(makeIconMenu())
        appMenu.addItem(.separator())
        let test = NSMenuItem(title: "发送离线测试通知", action: #selector(testNotification), keyEquivalent: "t")
        test.keyEquivalentModifierMask = [.command, .shift]
        test.target = self
        appMenu.addItem(test)
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "退出 SYSU NetAuth", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appItem.submenu = appMenu
        NSApp.mainMenu = menu
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 780, height: 490),
                          styleMask: [.titled, .closable, .miniaturizable], backing: .buffered, defer: false)
        window.title = "SYSU NetAuth" + (demo ? " · 离线模拟" : " · macOS")
        username.placeholderString = "请输入 NetID"
        password.placeholderString = "请输入密码"
        username.identifier = NSUserInterfaceItemIdentifier("netid")
        password.identifier = NSUserInterfaceItemIdentifier("password")
        autoAuth.state = .on
        login.toolTip = "当前系统用户进入桌面后运行；不影响后台服务自动认证。"
        notify.toolTip = "需要界面程序驻留菜单栏，退出程序后不发送通知。"
        interface.addItem(withTitle: "自动探测有线网卡")
        manual.target = self
        manual.action = #selector(selectMode)
        interface.isEnabled = false
        let service = NSTextField(labelWithString: "后台服务独立运行，退出界面不影响认证")
        service.font = .systemFont(ofSize: 11)
        service.textColor = .secondaryLabelColor
        let controls = NSStackView(views: [button("保存设置", #selector(save)),
                                          button("重新连接", #selector(connect)),
                                          button("断开连接", #selector(disconnect))])
        controls.spacing = 8
        buttons[1].keyEquivalent = "\r"
        let left = column([row("NetID", username), row("密码", password), row("网卡", interface),
                           manual, autoAuth, service, NSTextField(labelWithString: "界面与提醒"), login, hidden, notify, controls])
        status.font = .boldSystemFont(ofSize: 20)
        status.textColor = .secondaryLabelColor
        details.preferredMaxLayoutWidth = 308
        network.preferredMaxLayoutWidth = 308
        footer.preferredMaxLayoutWidth = 740
        let refresh = button("刷新状态", #selector(refresh))
        let right = column([status, details, network, refresh, button("查看日志", #selector(viewDiagnostics)), button("导出诊断信息…", #selector(exportDiagnostics)), button("安装／更新后台服务…", #selector(installService))], spacing: 14)
        let leftBox = box("账户与认证", left)
        let rightBox = box("网络状态", right)
        let panes = NSStackView(views: [leftBox, rightBox])
        panes.distribution = .fillEqually
        panes.alignment = .top
        leftBox.heightAnchor.constraint(equalTo: panes.heightAnchor).isActive = true
        rightBox.heightAnchor.constraint(equalTo: panes.heightAnchor).isActive = true
        panes.spacing = 12
        panes.heightAnchor.constraint(equalToConstant: 410).isActive = true
        footer.font = .systemFont(ofSize: 11)
        footer.textColor = .secondaryLabelColor
        footer.stringValue = demo ? "离线模拟 · 所有操作均不访问校园网" : "首次安装或更新服务需要管理员授权。关闭窗口后程序驻留菜单栏。"
        let root = column([panes, footer], spacing: 12)
        root.translatesAutoresizingMaskIntoConstraints = false
        window.contentView!.addSubview(root)
        NSLayoutConstraint.activate([
            root.leadingAnchor.constraint(equalTo: window.contentView!.leadingAnchor, constant: 18),
            root.trailingAnchor.constraint(equalTo: window.contentView!.trailingAnchor, constant: -18),
            root.topAnchor.constraint(equalTo: window.contentView!.topAnchor, constant: 16),
            panes.widthAnchor.constraint(equalTo: root.widthAnchor)
        ])
        window.center()
        configureDesktop()
        if shouldStartHidden() { NSApp.setActivationPolicy(.accessory) }
        else { showWindow() }
        runAction(nil, authorized: !demo)
    }
    @objc func installService() {
        if demo { footer.stringValue = "离线模拟：未安装或启动任何服务。"; return }
        guard let script = Bundle.main.resourceURL?.appendingPathComponent("install-macos.sh"),
              FileManager.default.fileExists(atPath: script.path) else {
            footer.stringValue = "应用包不完整，请重新构建。"; return
        }
        let alert = NSAlert()
        alert.messageText = "安装并启动后台认证服务？"
        alert.informativeText = "将安装 CLI、launchd 服务与配置目录。macOS 将请求管理员授权。已有服务会被更新。"
        alert.addButton(withTitle: "安装")
        alert.addButton(withTitle: "取消")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        setBusy(true)
        DispatchQueue.global(qos: .userInitiated).async {
            let result = Result { try Client(executable: URL(fileURLWithPath: "/bin/sh"))
                .execute(arguments: [script.path, String(getuid())], authorized: true) }
            DispatchQueue.main.async {
                self.setBusy(false)
                switch result {
                case .success:
                    self.footer.stringValue = "后台服务已安装；请填写账号后保存设置。"
                    self.runAction(nil, authorized: true)
                case .failure(let error): self.footer.stringValue = String(error.localizedDescription.prefix(240))
                }
            }
        }
    }
    @objc func selectMode() { interface.isEnabled = manual.state == .on && !busy }
    @objc func refresh() { runAction(.status, authorized: !demo) }
    @objc func connect() {
        if !username.stringValue.isEmpty || !password.stringValue.isEmpty {
            saveSettings(connectAfter: true)
        } else {
            notices.connectRequested()
            runAction(.authenticate, authorized: !demo)
        }
    }
    @objc func disconnect() { runAction(.logoff, authorized: !demo) }
    @objc func save() { saveSettings(connectAfter: false) }
    func saveSettings(connectAfter: Bool) {
        guard !username.stringValue.trimmingCharacters(in: .whitespaces).isEmpty,
              !password.stringValue.isEmpty else {
            footer.stringValue = "请填写 NetID 与密码。"
            return
        }
        guard manual.state != .on || detected.indices.contains(interface.indexOfSelectedItem) else {
            footer.stringValue = "没有可手动选择的以太网接口。"
            return
        }
        guard demo || preferencesLoaded else { footer.stringValue = "请先更新后台服务并刷新状态，读取已有设置。"; return }
        var settings = Settings()
        settings.username = username.stringValue
        settings.password = password.stringValue
        settings.retry_interval = retryInterval
        settings.auto_auth = autoAuth.state == .on
        settings.launch_gui_on_login = login.state == .on
        settings.hide_window_on_login = hidden.state == .on
        settings.desktop_notify = notify.state == .on
        if manual.state == .on {
            settings.iface_mode = "manual"
            settings.iface = detected[interface.indexOfSelectedItem].id
        }
        if demo {
            footer.stringValue = "离线模拟：设置校验通过，未保存凭据或联系服务。"
            return
        }
        setBusy(true)
        if connectAfter { notices.connectRequested() }
        DispatchQueue.global(qos: .userInitiated).async {
            let result = Result { () throws -> Snapshot? in
                let data = try self.client.save(settings, authorized: true, connect: connectAfter)
                if connectAfter { return try JSONDecoder().decode(Snapshot.self, from: data) }
                return nil
            }
            DispatchQueue.main.async {
                self.setBusy(false)
                switch result {
                case .success(let snapshot):
                    self.footer.stringValue = "设置已保存至后台服务。"
                    if let snapshot = snapshot { self.applySnapshot(snapshot) }
                case .failure(let error):
                    self.notices.cancelConnectRequest()
                    self.footer.stringValue = String(error.localizedDescription.prefix(240))
                }
            }
        }
    }
    func setBusy(_ value: Bool) {
        busy = value
        if value { statusGeneration += 1; noticeTimer?.invalidate() }
        buttons.forEach { $0.isEnabled = !value }
        menuActions.forEach { $0.isEnabled = !value }
        [username, password].forEach { $0.isEnabled = !value }
        manual.isEnabled = !value
        autoAuth.isEnabled = !value
        selectMode()
    }
    func runAction(_ action: Action?, authorized: Bool) {
        guard !busy else { return }
        setBusy(true)
        DispatchQueue.global(qos: .userInitiated).async {
            var preferences: Preferences?
            let result = Result { () throws -> Snapshot in
                if !self.demo && !self.preferencesLoaded {
                    preferences = try JSONDecoder().decode(Preferences.self, from: self.client.run(.preferences, authorized: authorized))
                }
                let data = try self.client.run(action ?? .status, authorized: authorized)
                return try JSONDecoder().decode(Snapshot.self, from: data)
            }
            DispatchQueue.main.async {
                switch result {
                case .success(let snapshot):
                    if let prefs = preferences {
                        self.retryInterval = prefs.retry_interval
                        self.autoAuth.state = prefs.auto_auth ? .on : .off
                        self.manual.state = prefs.iface_mode == "manual" ? .on : .off
                        self.preferencesLoaded = true
                        self.applySnapshot(snapshot)
                        if let index = self.detected.firstIndex(where: { $0.id == prefs.iface }) { self.interface.selectItem(at: index) }
                    } else { self.applySnapshot(snapshot) }
                case .failure(let error):
                    if action == .authenticate { self.notices.cancelConnectRequest() }
                    self.noticeTimer?.invalidate()
                    self.updateStatusItem("unavailable")
                    self.status.stringValue = "操作未完成"
                    self.status.textColor = .systemOrange
                    let message = error.localizedDescription
                    if message.contains("service unavailable") {
                        self.status.stringValue = "服务未连接"
                        self.details.stringValue = "后台服务未运行或当前用户无权访问。首次使用请安装服务；已安装时点击“刷新状态”授权读取。"
                    } else if message.contains("需要配置 NetID 和密码") {
                        self.status.stringValue = "需要配置账号"
                        self.details.stringValue = "请填写 NetID 和密码，保存设置后重新连接。"
                    } else if message.contains("canceled") || message.contains("cancelled") || message.contains("取消") {
                        self.status.stringValue = "操作已取消"
                        self.details.stringValue = "可稍后重试并完成系统授权。"
                    } else {
                        self.details.stringValue = String(message.prefix(180))
                    }
                }
                self.setBusy(false)
            }
        }
    }
    func showNetwork(_ adapter: Adapter?) {
        guard let a = adapter else { network.stringValue = "未发现以太网接口，请连接有线网卡。"; return }
        func display(_ value: String?) -> String { guard let value = value, !value.isEmpty else { return "—" }; return value }
        let mac = a.mac?.map { String(format: "%02X", $0) }.joined(separator: ":") ?? "—"
        network.stringValue = "网卡    \(a.name)\n驱动    \(a.driver ?? "—")\nIPv4    \(a.ipv4.isEmpty ? "—" : a.ipv4)\nMAC     \(mac)\n网关    \(display(a.gateway))\nDNS     \(display(a.dns))"
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }
}
let application = NSApplication.shared
let delegate = AppDelegate()
application.delegate = delegate
application.setActivationPolicy(.regular)
application.run()
