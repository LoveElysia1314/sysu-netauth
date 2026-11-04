# 性能测量

后台服务和 GUI 应分别测量。记录系统、架构、程序版本、网卡和认证状态，并将启动、认证、联网探测及更新下载与稳定运行阶段分开。

## Windows

分别采样服务和界面进程，例如持续十分钟：

```powershell
./scripts/measure-windows.ps1 -ProcessName sysu_netauth_service -Seconds 600
./scripts/measure-windows.ps1 -ProcessName sysu_netauth_gui -Seconds 600
```

待命、认证成功后持续续期和反复恢复可分别采样。比较版本时保持机器、网卡和采样时长一致。

## Linux

以下脚本在隔离网络命名空间中测量待命状态和模拟认证后的服务：

```sh
sh scripts/benchmark-linux.sh 600
sh scripts/benchmark-linux-authenticated.sh 600
```

需要 iproute2、util-linux，以及创建用户/网络命名空间的权限。认证场景使用模拟的 EAP-MD5 认证端，测量结果反映该环境中的进程开销。

结果应注明 RSS、CPU 统计口径、写盘量和采样时长。CPU 低于采样分辨率时，应按分辨率解释显示值。
