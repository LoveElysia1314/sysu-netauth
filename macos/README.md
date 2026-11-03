# macOS 原生 GUI（开发预览）

AppKit 界面采用“账户与连接 / 网络状态”双栏布局，支持账号配置、选卡、自动认证、保存、重新连接和断开。应用常驻菜单栏，可设置登录启动、登录时隐藏窗口和状态通知。更新界面尚未移植。

## 构建与验证

需要 Rust stable 和 Swift 5.9+。标准 XCTest 需要完整 Xcode；独立离线检查可使用 Xcode Command Line Tools。无第三方 Swift 依赖。

```sh
swift test --package-path macos
sh scripts/test-macos-gui-offline.sh
sh scripts/build-macos-gui.sh
open dist/SYSUNetAuth.app
```

应用包包含 Rust CLI、认证服务和 XPC helper，版本读取 Rust 包版本。当前使用 ad-hoc 签名，尚无 Developer ID 签名或公证，GUI 应用包未纳入正式发行附件。

## 安装和授权

首次使用时点击“安装后台服务”，完成系统管理员授权。macOS 13+ 的 helper 绑定安装用户 UID 和 GUI 的代码签名哈希，GUI 同时校验 helper 签名。helper 只接受状态、认证、断开、保存设置及保存并连接等固定请求，服务原有 IPC 继续仅允许 root 访问。

二进制更新后需要重新安装后台组件并授权。macOS 11/12 使用单次管理员授权调用，登录项不可用。安装后可运行 `sh scripts/test-macos-helper-rejection.sh`，检查不同签名客户端是否被拒绝；成功读取 GUI 状态可作为正向对照。

界面暂不回填服务中已有的账号配置。账号字段均留空时，“重新连接”使用已有设置；编辑设置时需要填写完整的 NetID 和密码。密码不进入命令行参数。

## 连接和通知

“保存并连接”由服务一次完成。手动选卡保存设备 ID（MAC 地址），接口改名或列表重排后仍保留所选设备；拔出后保留“未连接”项。网卡和连接状态由同一份服务快照更新。

通知默认关闭，需要主动申请系统授权。自动断线在观测后的 5 秒内恢复时静默处理；持续中断提醒一次，恢复后再提示一次。主动连接成功正常通知，休眠和手动断开不触发断线提醒。通知仅含通用状态，不显示账号或后台错误详情。

macOS 13+ 每 5 秒读取一次 helper 状态，手动操作期间暂停轮询。关闭窗口后保留菜单栏，退出 GUI 后认证服务仍可独立运行。登录项通过 SMAppService 注册；注册后应保持应用路径稳定。“启动时隐藏窗口”只影响登录启动。

## 图标与测试边界

菜单栏提供系统单色和彩色两种图标模式，同时用符号、文字和无障碍描述区分六种状态。图标源为 [`assets/icons/source.json`](../assets/icons/source.json)，生成说明见[共享资产文档](../assets/icons/README.md)。

离线检查覆盖 JSON 契约、模拟 CLI、超时、请求边界、设备选择及通知防抖。CI 运行 XCTest、离线检查和完整应用构建。真实校园网认证、热插拔、睡眠恢复及真实注销重登仍需要设备验证；离线检查不能替代这些场景。
