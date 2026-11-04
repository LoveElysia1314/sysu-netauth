# Linux 安装与服务管理

Linux x64 提供命令行和 systemd 后台服务，需要 root 权限访问认证网卡。

## 安装与配置

从[发布页](https://github.com/LoveElysia1314/sysu-netauth/releases)下载 `linux_x64` 压缩包，解压后安装：

```sh
tar -xzf SYSUNetAuth_linux_x64_v*.tar.gz
cd sysu-netauth-linux-x64
sudo sh install-linux.sh
sysu_netauth interfaces
```

将账号写入一个仅本人可读的 `config.json`：

```json
{"username":"NetID","password":"密码","iface_mode":"auto","auto_auth":true}
```

```sh
chmod 600 config.json
sudo sysu_netauth config import config.json
sudo sysu_netauth status --json
sudo sysu_netauth watch --json
```

服务保存的配置位于 `/etc/sysu-netauth/config.json`，日志和缓存位于 `/var/lib/sysu-netauth/`。程序负责 802.1X 认证，IP 地址和 DHCP 由系统网络管理器处理。

## 服务管理

```sh
sudo systemctl status sysu-netauth
sudo systemctl restart sysu-netauth
sudo systemctl disable --now sysu-netauth
```

重新启用服务：`sudo systemctl enable --now sysu-netauth`。查看连接日志：`sudo tail -n 40 /var/lib/sysu-netauth/service.log`。

服务自动检查正式版更新。运行 `sudo sysu_netauth status` 时，会显示可用版本及发布说明链接；每个新版本首次被发现时也会写入连接日志。`--json` 输出继续提供结构化的 `update` 字段。当前压缩包通过安装脚本部署，升级时下载新版并再次安装；apt、dnf 等包管理器尚未管理此安装。

升级时解压新版本并再次运行 `install-linux.sh`，安装脚本会停止服务、替换程序并重新启动。

手动选卡、修改密码和直接认证等操作见[命令行与配置](CLI.md)。
