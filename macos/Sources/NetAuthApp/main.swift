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
    var networkFields: [NSTextField] = []
    let footer = NSTextField(wrappingLabelWithString: "")
    var buttons: [NSButton] = []
    var detected: [Adapter] = []
    var busy = false
    var uninstallCompleted = false
    var savedAccount: Settings?
    var currentState = "idle"
    var authenticationButton: NSButton?
    var savedAutoAuth = true
    var authenticationPreferenceInFlight = false
    let login = NSButton(checkboxWithTitle: "开机后启动界面程序", target: nil, action: nil)
    let hidden = NSButton(checkboxWithTitle: "开机后隐藏配置窗口", target: nil, action: nil)
    let notify = NSButton(checkboxWithTitle: "认证状态变化后通知", target: nil, action: nil)
    let autoCheck = NSButton(checkboxWithTitle: "自动检查更新", target: nil, action: nil)
    let notifyUpdates = NSButton(checkboxWithTitle: "有新版本时通知", target: nil, action: nil)
    let updateLabel = NSTextField(labelWithString: "尚未检查更新")
    var updateCheckButton: NSButton?
    var updateReleaseButton: NSButton?
    var updateNotificationsEnabled = true
    var savedAutoUpdateCheck = true
    var savedUpdateNotify = true
    var updatePreferencesInFlight = false
    var statusItem: NSStatusItem?
    var menuStatus: NSMenuItem?
    var menuActions: [NSMenuItem] = []
    var pollTimer: Timer?
    var noticeTimer: Timer?
    var pollInFlight = false
    var statusGeneration = 0
    var notices = StateNotifications()
    var connectionRequest: UInt64?
    let notificationsOnly = CommandLine.arguments.contains("--notifications")
    var notificationLease: NotificationAgentLease?
    var interfaceLease: NotificationAgentLease?
    var showWindowObserver: NSObjectProtocol?
    var releaseUpdate: ReleaseUpdate?
    var updateRequestInFlight = false
    var attemptedUpdateVersion = ""
    var updateInfoItems: [NSMenuItem] = []
    var updateReleaseItems: [NSMenuItem] = []
    var updateIgnoreItems: [NSMenuItem] = []
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
        stack.distribution = .fill
        text.setContentHuggingPriority(.required, for: .horizontal)
        control.setContentHuggingPriority(.defaultLow, for: .horizontal)
        control.widthAnchor.constraint(greaterThanOrEqualToConstant: 252).isActive = true
        return stack
    }
    func button(_ title: String, _ action: Selector) -> NSButton {
        let b = NSButton(title: title, target: self, action: action)
        b.bezelStyle = .rounded
        buttons.append(b)
        return b
    }
    func box(_ title: String, _ content: NSView, fillsContent: Bool = true) -> NSView {
        let box = NSBox()
        box.titlePosition = .noTitle
        box.boxType = .custom
        box.borderWidth = 0
        box.borderColor = .clear
        box.cornerRadius = 8
        box.isTransparent = false
        box.fillColor = NSColor.controlBackgroundColor.blended(withFraction: 0.035, of: .labelColor) ?? .controlBackgroundColor
        box.setAccessibilityLabel(title)
        box.contentViewMargins = NSSize(width: 14, height: 14)
        box.contentView = NSView()
        content.translatesAutoresizingMaskIntoConstraints = false
        box.contentView!.addSubview(content)
        NSLayoutConstraint.activate([
            content.leadingAnchor.constraint(equalTo: box.contentView!.leadingAnchor),
            content.trailingAnchor.constraint(equalTo: box.contentView!.trailingAnchor),
            content.topAnchor.constraint(equalTo: box.contentView!.topAnchor),
            fillsContent ? content.bottomAnchor.constraint(equalTo: box.contentView!.bottomAnchor) : content.bottomAnchor.constraint(lessThanOrEqualTo: box.contentView!.bottomAnchor)
        ])
        let heading = NSTextField(labelWithString: title)
        heading.font = .boldSystemFont(ofSize: 13)
        let group = column([heading, box], spacing: 10)
        box.widthAnchor.constraint(equalTo: group.widthAnchor).isActive = true
        return group
    }
    /// Equal tracks derive their widths from the parent, just like a QGridLayout.
    func grid(_ rows: [[NSView]], columns: Int, rowSpacing: CGFloat = 10) -> NSStackView {
        let result = column([], spacing: rowSpacing)
        for cells in rows {
            precondition(cells.count == columns)
            let row = NSStackView(views: cells)
            row.orientation = .horizontal
            row.distribution = .fillEqually
            row.alignment = .centerY
            row.spacing = 8
            result.addArrangedSubview(row)
            row.widthAnchor.constraint(equalTo: result.widthAnchor).isActive = true
        }
        return result
    }
    @objc func quitProgram() { NSApp.terminate(nil) }
    func pane(_ body: NSView, actions: NSView) -> NSView {
        let view = NSView()
        for child in [body, actions] {
            child.translatesAutoresizingMaskIntoConstraints = false
            view.addSubview(child)
        }
        NSLayoutConstraint.activate([
            body.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            body.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            body.topAnchor.constraint(equalTo: view.topAnchor),
            body.bottomAnchor.constraint(equalTo: actions.topAnchor, constant: -12),
            actions.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            actions.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            actions.bottomAnchor.constraint(equalTo: view.bottomAnchor)
        ])
        return view
    }
    func applicationDidFinishLaunching(_ notification: Notification) {
        if notificationsOnly {
            do {
                guard let lease = try NotificationAgentLease.acquire() else { NSApp.terminate(nil); return }
                notificationLease = lease
                configureNotificationAgent()
            } catch { NSApp.terminate(nil) }
            return
        }
        if !demo {
            do {
                guard let lease = try NotificationAgentLease.acquire(name: "interface") else {
                    DistributedNotificationCenter.default().postNotificationName(.init("io.github.peterrobot.sysu-netauth.show-window"), object: String(getuid()), userInfo: nil, deliverImmediately: true)
                    NSApp.terminate(nil)
                    return
                }
                interfaceLease = lease
            } catch { NSApp.terminate(nil); return }
        }
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
        appMenu.addItem(makeUpdateMenu())
        appMenu.addItem(.separator())
        let install = NSMenuItem(title: "安装／更新服务…", action: #selector(installService), keyEquivalent: "")
        install.target = self
        appMenu.addItem(install)
        let uninstall = NSMenuItem(title: "卸载 SYSU NetAuth…", action: #selector(uninstallApplication), keyEquivalent: "")
        uninstall.target = self
        appMenu.addItem(uninstall)
        appMenu.addItem(withTitle: "退出 SYSU NetAuth", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appItem.submenu = appMenu
        NSApp.mainMenu = menu
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 780, height: 528),
                          styleMask: [.titled, .closable, .miniaturizable], backing: .buffered, defer: false)
        window.title = "SYSU NetAuth · v" + (Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0.7.0") + (demo ? " · 离线模拟" : "")
        username.placeholderString = "请输入 NetID"
        password.placeholderString = "请输入密码"
        username.identifier = NSUserInterfaceItemIdentifier("netid")
        password.identifier = NSUserInterfaceItemIdentifier("password")
        autoAuth.state = .on
        login.toolTip = "当前系统用户进入桌面后运行；不影响后台服务自动认证。"
        notify.toolTip = "独立通知助手负责发送，退出状态程序后仍然有效。"
        interface.addItem(withTitle: "自动探测有线网卡")
        manual.target = self
        manual.action = #selector(selectMode)
        interface.isEnabled = false
        autoCheck.state = .on
        notifyUpdates.state = .on
        authenticationButton = button("认证", #selector(connect))
        authenticationButton?.keyEquivalent = "\r"
        let controls = grid([[authenticationButton!, button("断开连接", #selector(disconnect)), button("退出程序", #selector(quitProgram))]], columns: 3)
        username.delegate = self; password.delegate = self
        interface.target = self; interface.action = #selector(accountChanged)
        autoAuth.target = self; autoAuth.action = #selector(changeAutoAuthentication)
        let accountRows = [row("NetID", username), row("密码", password), row("网卡", interface)]
        let accountChecks = grid([[manual, autoAuth]], columns: 2)
        let account = column(accountRows + [accountChecks], spacing: 12)
        (accountRows + [accountChecks]).forEach { $0.widthAnchor.constraint(equalTo: account.widthAnchor).isActive = true }
        let accountBox = box("账户与认证", pane(account, actions: controls))
        let preferences = grid([[login, hidden], [notify, autoCheck], [notifyUpdates, NSView()]], columns: 2)
        let preferencesBox = box("常规设置", preferences, fillsContent: false)
        status.font = .boldSystemFont(ofSize: 20)
        status.textColor = .secondaryLabelColor
        details.preferredMaxLayoutWidth = 308
        details.maximumNumberOfLines = 2
        details.isHidden = true
        var rows: [[NSView]] = []
        for title in ["网卡", "驱动", "IPv4", "MAC", "网关", "DNS"] {
            let label = NSTextField(labelWithString: title)
            label.textColor = .secondaryLabelColor
            let value = NSTextField(wrappingLabelWithString: "—")
            value.isSelectable = true
            value.maximumNumberOfLines = 2
            value.preferredMaxLayoutWidth = 242
            networkFields.append(value)
            rows.append([label, value])
        }
        let networkGrid = NSGridView(views: rows)
        networkGrid.columnSpacing = 12
        networkGrid.rowSpacing = 5
        networkGrid.column(at: 0).width = 40
        let right = column([status, details, networkGrid], spacing: 12)
        let rightBox = box("网络状态", right, fillsContent: false)
        let panes = NSStackView(views: [accountBox, rightBox])
        panes.distribution = .fillEqually
        panes.alignment = .top
        accountBox.heightAnchor.constraint(equalTo: panes.heightAnchor).isActive = true
        rightBox.heightAnchor.constraint(equalTo: panes.heightAnchor).isActive = true
        panes.spacing = 12
        updateLabel.stringValue = "当前版本 v" + (Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0.7.0")
        updateLabel.textColor = .secondaryLabelColor
        updateLabel.lineBreakMode = .byTruncatingTail
        updateReleaseButton = NSButton(title: "更新日志", target: self, action: #selector(openChangelog))
        updateCheckButton = NSButton(title: "检查更新", target: self, action: #selector(checkForUpdates))
        let tools = grid([[updateCheckButton!, button("运行日志", #selector(viewDiagnostics)), updateReleaseButton!]], columns: 3)
        updateLabel.setContentHuggingPriority(.required, for: .vertical)
        updateLabel.setContentCompressionResistancePriority(.required, for: .vertical)
        updateLabel.maximumNumberOfLines = 1
        let information = column([updateLabel, tools], spacing: 12)
        updateLabel.widthAnchor.constraint(equalTo: information.widthAnchor).isActive = true
        tools.widthAnchor.constraint(equalTo: information.widthAnchor).isActive = true
        let informationBox = box("应用信息", information, fillsContent: false)
        let lowerPanes = NSStackView(views: [preferencesBox, informationBox])
        lowerPanes.distribution = .fillEqually
        lowerPanes.alignment = .top
        lowerPanes.spacing = 12
        preferencesBox.heightAnchor.constraint(equalTo: lowerPanes.heightAnchor).isActive = true
        informationBox.heightAnchor.constraint(equalTo: lowerPanes.heightAnchor).isActive = true
        footer.preferredMaxLayoutWidth = 740
        footer.maximumNumberOfLines = 1
        footer.lineBreakMode = .byTruncatingTail
        footer.font = .systemFont(ofSize: 11)
        footer.textColor = .secondaryLabelColor
        footer.stringValue = demo ? "离线模拟 · 所有操作均不访问校园网" : ""
        let root = column([panes, lowerPanes, footer], spacing: 16)
        root.translatesAutoresizingMaskIntoConstraints = false
        window.contentView!.addSubview(root)
        NSLayoutConstraint.activate([
            root.leadingAnchor.constraint(equalTo: window.contentView!.leadingAnchor, constant: 18),
            root.trailingAnchor.constraint(equalTo: window.contentView!.trailingAnchor, constant: -18),
            root.topAnchor.constraint(equalTo: window.contentView!.topAnchor, constant: 16),
            root.bottomAnchor.constraint(lessThanOrEqualTo: window.contentView!.bottomAnchor, constant: -12),
            panes.widthAnchor.constraint(equalTo: root.widthAnchor),
            lowerPanes.widthAnchor.constraint(equalTo: root.widthAnchor)
        ])
        window.contentView!.layoutSubtreeIfNeeded()
        let fittedHeight = ceil(root.fittingSize.height) + 28
        window.setContentSize(NSSize(width: 780, height: fittedHeight))
        window.center()
        configureDesktop()
        if shouldStartHidden() { NSApp.setActivationPolicy(.accessory) }
        else { showWindow() }
        runAction(nil, authorized: !demo)
        if demo, let directory = ProcessInfo.processInfo.environment["NETAUTH_PREVIEW_DIR"] {
            DispatchQueue.main.asyncAfter(deadline: .now() + 2) {
                self.capturePreview(directory: directory)
            }
        }
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
                    self.footer.stringValue = "后台服务已安装；请填写账户后点击“认证”。"
                    self.runAction(nil, authorized: true)
                case .failure(let error): self.footer.stringValue = String(error.localizedDescription.prefix(240))
                }
            }
        }
    }
    @objc func selectMode() { interface.isEnabled = manual.state == .on && !busy; updateAuthenticationButton() }
    @objc func refresh() { runAction(.status, authorized: !demo) }
    @objc func connect() {
        guard !busy, !authenticationPreferenceInFlight, !updatePreferencesInFlight, currentState != "authenticating" else { return }
        saveSettings(connectAfter: true)
    }
    @objc func disconnect() { runAction(.logoff, authorized: !demo) }
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
        guard demo || preferencesLoaded else { footer.stringValue = "请先更新后台服务，读取已有设置。"; return }
        var settings = Settings()
        settings.username = username.stringValue
        settings.password = password.stringValue
        settings.retry_interval = retryInterval
        settings.auto_auth = autoAuth.state == .on
        settings.launch_gui_on_login = login.state == .on
        settings.hide_window_on_login = hidden.state == .on
        settings.desktop_notify = notify.state == .on
        settings.auto_update_check = autoCheck.state == .on
        settings.update_notify = notifyUpdates.state == .on
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
            var persistedAccount: Settings?
            let result = Result { () throws -> Snapshot? in
                let data = try self.client.save(settings, authorized: true, connect: connectAfter)
                if connectAfter { return try JSONDecoder().decode(Snapshot.self, from: data) }
                return nil
            }
            if case .failure = result, #available(macOS 13, *) {
                if let data = try? Helper.call("get_config") { persistedAccount = try? JSONDecoder().decode(Settings.self, from: data) }
            }
            DispatchQueue.main.async {
                if let account = persistedAccount { self.savedAccount = account }
                self.setBusy(false)
                switch result {
                case .success(let snapshot):
                    self.savedAccount = settings
                    self.footer.stringValue = "账户已保存，认证请求已提交。"
                    self.updateAuthenticationButton()
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
        autoAuth.isEnabled = !value && !authenticationPreferenceInFlight
        autoCheck.isEnabled = !value && !updatePreferencesInFlight && (demo || preferencesLoaded)
        notifyUpdates.isEnabled = !value && !updatePreferencesInFlight && (demo || preferencesLoaded)
        selectMode()
        updateAuthenticationButton()
    }
    func runAction(_ action: Action?, authorized: Bool) {
        guard !busy else { return }
        setBusy(true)
        DispatchQueue.global(qos: .userInitiated).async {
            var preferences: Preferences?
            var account: Settings?
            let result = Result { () throws -> Snapshot in
                if !self.demo && !self.preferencesLoaded {
                    preferences = try JSONDecoder().decode(Preferences.self, from: self.client.run(.preferences, authorized: authorized))
                    if #available(macOS 13, *) { account = try JSONDecoder().decode(Settings.self, from: Helper.call("get_config")) }
                }
                let data = try self.client.run(action ?? .status, authorized: authorized)
                return try JSONDecoder().decode(Snapshot.self, from: data)
            }
            DispatchQueue.main.async {
                switch result {
                case .success(let snapshot):
                    if let prefs = preferences {
                        if let account = account {
                            self.savedAccount = account
                            self.username.stringValue = account.username
                            self.password.stringValue = account.password
                        }
                        self.retryInterval = prefs.retry_interval
                        self.autoAuth.state = prefs.auto_auth ? .on : .off
                        self.savedAutoAuth = prefs.auto_auth
                        self.autoCheck.state = (prefs.auto_update_check ?? true) ? .on : .off
                        self.notifyUpdates.state = (prefs.update_notify ?? true) && self.localUpdateNotifications ? .on : .off
                        self.savedAutoUpdateCheck = self.autoCheck.state == .on
                        self.savedUpdateNotify = self.notifyUpdates.state == .on
                        self.manual.state = prefs.iface_mode == "manual" ? .on : .off
                        self.preferencesLoaded = true
                        self.applySnapshot(snapshot)
                        if self.manual.state == .on {
                            self.refreshAdapters(AdapterSelection.restore(snapshot.adapters, preferredID: prefs.iface))
                        }
                        if let index = self.detected.firstIndex(where: { $0.id == prefs.iface }) { self.interface.selectItem(at: index) }
                    } else { self.applySnapshot(snapshot) }
                case .failure(let error):
                    if action == .authenticate { self.notices.cancelConnectRequest() }
                    self.noticeTimer?.invalidate()
                    self.updateStatusItem("unavailable")
                    self.details.isHidden = false
                    self.status.stringValue = "操作未完成"
                    self.status.textColor = .systemOrange
                    let message = error.localizedDescription
                    if message.contains("service unavailable") {
                        self.status.stringValue = "服务未连接"
                        self.details.stringValue = "后台服务未运行或当前用户无权访问。请从应用菜单安装或更新服务。"
                    } else if message.contains("需要配置 NetID 和密码") {
                        self.status.stringValue = "需要配置账号"
                        self.details.stringValue = "请填写 NetID 和密码，点击“认证”。"
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
        func display(_ value: String?) -> String { guard let value = value, !value.isEmpty else { return "—" }; return value }
        let mac = adapter?.mac?.map { String(format: "%02X", $0) }.joined(separator: ":")
        let values = [display(adapter?.name), display(adapter?.driver), display(adapter?.ipv4), display(mac), display(adapter?.gateway), display(adapter?.dns)]
        for (field, value) in zip(networkFields, values) { field.stringValue = value }
    }
    func capturePreview(directory: String) {
        guard demo, let view = window.contentView else { return }
        do {
            let folder = URL(fileURLWithPath: directory)
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            let name = ProcessInfo.processInfo.environment["NETAUTH_PREVIEW_STYLE"] ?? "light"
            // Text changes after checking must never move the label outside its group.
            view.layoutSubtreeIfNeeded()
            let original = updateLabel.convert(updateLabel.bounds, to: view)
            let originalText = updateLabel.stringValue
            for text in ["正在检查更新…", "当前版本 v0.7.0 · 已是最新", "检查失败，请稍后重试", "存在新版本：v9999.9999.9999"] {
                showUpdateLabel(text)
                view.layoutSubtreeIfNeeded()
                let frame = updateLabel.convert(updateLabel.bounds, to: view)
                precondition(abs(frame.minX - original.minX) < 0.5 && abs(frame.minY - original.minY) < 0.5,
                             "Update label moved after changing its text")
                precondition(abs(frame.height - original.height) < 0.5, "Update label stretched vertically")
            }
            showUpdateLabel(originalText)
            var output: Data?
            window.effectiveAppearance.performAsCurrentDrawingAppearance {
                view.layoutSubtreeIfNeeded()
                view.display()
                guard let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return }
                view.cacheDisplay(in: view.bounds, to: bitmap)
                guard let source = bitmap.cgImage,
                      let background = NSColor.windowBackgroundColor.usingColorSpace(.deviceRGB),
                      let context = CGContext(data: nil, width: source.width, height: source.height, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return }
                let bounds = CGRect(x: 0, y: 0, width: source.width, height: source.height)
                context.setFillColor(red: background.redComponent, green: background.greenComponent, blue: background.blueComponent, alpha: 1)
                context.fill(bounds)
                context.draw(source, in: bounds)
                if let image = context.makeImage() {
                    output = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:])
                }
            }
            guard let data = output else { throw ClientError.failed("无法生成界面预览") }
            try data.write(to: folder.appendingPathComponent("macos-\(name).png"))
            NSApp.terminate(nil)
        } catch { fputs("Preview: \(error)\n", stderr); exit(1) }
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }
}
if CommandLine.arguments.contains("--check-helper") {
    guard #available(macOS 13, *) else { fputs("helper requires macOS 13+\n", stderr); exit(2) }
    do {
        _ = try JSONDecoder().decode(Snapshot.self, from: Helper.call("status"))
        _ = try JSONDecoder().decode(Preferences.self, from: Helper.call("preferences"))
        _ = try JSONDecoder().decode(Settings.self, from: Helper.call("get_config"))
        print("PASS: signed GUI connected to installed helper; status and preferences decoded")
        exit(0)
    } catch { fputs("helper check failed: \(error.localizedDescription)\n", stderr); exit(1) }
}
let application = NSApplication.shared
if ProcessInfo.processInfo.environment["NETAUTH_CLI"]?.hasSuffix("mock-cli.py") == true,
   let style = ProcessInfo.processInfo.environment["NETAUTH_PREVIEW_STYLE"] {
    application.appearance = NSAppearance(named: style == "dark" ? .darkAqua : .aqua)
}
let delegate = AppDelegate()
application.delegate = delegate
application.setActivationPolicy(delegate.notificationsOnly ? .accessory : .regular)
application.run()
