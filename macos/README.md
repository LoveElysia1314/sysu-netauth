# macOS 界面开发

界面使用 AppKit，按“账户与认证、网络状态、常规设置、应用信息”分为四组，与 Windows 对应。组标题与背景容器独立排列，保留标题间距；窗口按内容计算高度。

安装与日常使用见 [macOS 指南](../docs/MACOS.md)，本文说明构建、授权和验证方法。

## 构建

需要 Rust stable、Swift 5.9+ 和 Xcode Command Line Tools。运行 XCTest 需安装完整 Xcode。

```sh
swift test --package-path macos
sh scripts/test-macos-gui-offline.sh
sh scripts/build-macos-gui.sh
open dist/SYSUNetAuth.app
```

使用 `sh scripts/build-macos.sh` 生成当前架构的 PKG。应用包包含 CLI、认证服务和 XPC helper，版本读取 `Cargo.toml`。当前采用 ad-hoc 签名；首次打开的系统授权步骤见[安装指南](../docs/MACOS.md#首次打开时的安全提示)。

## 服务授权

PKG 将应用安装到 `/Applications/SYSUNetAuth.app`，并安装后台组件。安装时若没有桌面用户登录，首次使用可从应用菜单选择“安装／更新服务…”，完成管理员授权。升级和源码构建后的组件替换也通过该流程授权。

macOS 13+ 的 helper 绑定安装用户 UID 和应用签名哈希。界面与通知助手校验 helper 签名，helper 按固定操作列表转发请求；认证服务的 IPC 由 root 访问。macOS 11/12 通过单次管理员授权执行服务操作。

安装后验证真实 GUI 的 XPC 连接和签名拒绝行为：

```sh
/Applications/SYSUNetAuth.app/Contents/MacOS/SYSUNetAuth --check-helper
sh scripts/test-macos-helper-rejection.sh
```

第一项以当前用户和应用签名读取状态、非敏感设置；第二项验证不同签名的客户端会被拒绝。PKG 安装、升级测试包含这两项检查。

## 界面与通知

界面通过已授权的 helper 回填服务中保存的账户。修改账号后点击“认证”保存并使用，重新打开时恢复已保存内容；常规选项修改后立即保存。手动选卡保存设备 ID，列表变化时保留所选设备。网卡和认证状态来自同一份服务快照。

macOS 13+ 每 5 秒自动读取状态，手动操作期间暂停轮询。关闭窗口后程序驻留菜单栏；手动打开应用显示配置窗口，自动启动时遵循“开机后隐藏配置窗口”设置。

独立通知助手使用同一应用的 `--notifications` 模式运行，由用户会话的 LaunchAgent 启动，以文件锁保证单实例。认证通知采用 5 秒防抖；更新通知按版本去重。通知投递受系统通知权限和专注模式设置影响。

“应用信息”集中提供退出程序、检查更新、运行日志和更新日志。日志接口读取固定文件的末尾，保留最新记录并遮盖当前凭据；文本上限为 28 KiB。

## 图标与能耗

菜单栏图标用符号和颜色表达六种状态，支持系统单色和彩色样式。几何与配色统一维护在 [`assets/icons/source.json`](../assets/icons/source.json)，程序坞和 Finder 图标在构建时生成；详见[资产说明](../assets/icons/README.md)。

仅在状态、样式或归一化外观改变时替换图像，合并重复的外观通知。验证时分别采样前台窗口和隐藏驻留状态，暂停窗口采集，忽略首个累计样本：

```sh
top -pid <GUI_PID> -l 4 -s 5 -stats pid,command,cpu,time,threads,idlew,power
```

活动监视器的 12 小时能耗值包含历史数据；判断修复效果时结合当前 CPU 和连续采样。自动化检查与设备验证范围见[测试文档](../docs/VALIDATION.md)。

卸载入口位于应用菜单。GUI 先注销当前用户的登录启动，再经单独管理员授权执行卸载脚本；脚本卸载各用户通知任务、系统服务和 helper，按固定路径校验应用标识后移除应用。默认保留配置，`--purge` 可清理配置与日志。隔离文件系统测试覆盖保留、完整移除、错误参数、应用标识及符号链接拒绝；真实 PKG 安装与卸载在临时 CI 机器执行。
