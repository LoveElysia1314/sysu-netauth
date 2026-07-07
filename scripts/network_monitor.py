"""
网络连通性监控脚本
====================
每隔 5 秒 ping 公网地址，记录失败时间与原因。
用于测试 sysu-netauth 认证过程中是否存在间断断网。

用法:
    python scripts/network_monitor.py                          # 使用系统 ping（ICMP）
    python scripts/network_monitor.py --mode http              # 使用 HTTP 请求
    python scripts/network_monitor.py --target 223.5.5.5       # 自定义目标
    python scripts/network_monitor.py --log ping_log.csv       # 指定日志路径
"""

import argparse
import csv
import datetime
import subprocess
import sys
import time
from pathlib import Path


def _timestamp() -> str:
    return datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S.%f")[:-3]


def _ping_icmp(target: str) -> tuple[bool, str]:
    """使用系统 ping (ICMP) 检测连通性。"""
    try:
        proc = subprocess.run(
            ["ping", "-n", "1", "-w", "3000", target],
            capture_output=True,
            text=True,
            timeout=10,
            creationflags=subprocess.CREATE_NO_WINDOW,
        )
        if proc.returncode == 0:
            return True, ""
        # 提取关键错误信息
        lines = [l.strip() for l in proc.stdout.splitlines() if l.strip()]
        err_lines = [l.strip() for l in proc.stderr.splitlines() if l.strip()]
        detail = "; ".join(lines[-2:] + err_lines[-2:])
        return False, detail or f"ping 返回码 {proc.returncode}"
    except subprocess.TimeoutExpired:
        return False, "ping 超时 (10s)"
    except Exception as e:
        return False, f"异常: {e}"


def _ping_http(target: str) -> tuple[bool, str]:
    """使用 HTTP 请求 (TCP 443) 检测连通性。"""
    try:
        import urllib.request

        req = urllib.request.Request(f"http://{target}", method="HEAD")
        # 短超时，快速失败
        urllib.request.urlopen(req, timeout=5)
        return True, ""
    except urllib.error.URLError as e:
        return False, f"HTTP 错误: {e.reason}"
    except Exception as e:
        return False, f"HTTP 异常: {e}"


def main():
    parser = argparse.ArgumentParser(description="网络连通性监控")
    parser.add_argument(
        "--mode",
        choices=["icmp", "http"],
        default="icmp",
        help="检测方式: icmp（系统 ping）或 http（TCP 443 请求）",
    )
    parser.add_argument(
        "--target",
        default="223.5.5.5",
        help="检测目标，默认 223.5.5.5（阿里公共 DNS）",
    )
    parser.add_argument(
        "--interval",
        type=float,
        default=5.0,
        help="检测间隔（秒），默认 5",
    )
    parser.add_argument(
        "--log",
        default="",
        help="日志 CSV 路径，默认自动生成",
    )
    args = parser.parse_args()

    # 自动生成日志文件名
    log_path = (
        Path(args.log)
        if args.log
        else (Path.cwd() / f"ping_log_{datetime.date.today().isoformat()}.csv")
    )

    # HTTP 模式下补全 URL 语义
    target_display = args.target
    ping_fn = _ping_icmp if args.mode == "icmp" else _ping_http

    # 写 CSV 表头
    with open(log_path, "w", newline="", encoding="utf-8-sig") as f:
        writer = csv.writer(f)
        writer.writerow(["时间", "状态", "延迟(ms)", "详情"])
        f.flush()

    print(f"[{_timestamp()}] 开始监控 — 目标: {target_display}  方式: {args.mode}")
    print(f"[{_timestamp()}] 日志: {log_path.resolve()}")
    print(f"[{_timestamp()}] {'='*50}")
    print()

    ok_count = 0
    fail_count = 0
    first_fail_ts = ""

    try:
        while True:
            ts = _timestamp()
            ok, detail = ping_fn(args.target)

            if ok:
                ok_count += 1
                # 仅成功时不写日志（避免文件过大），每 60 次打印心跳
                if ok_count % 60 == 0:
                    print(f"[{ts}] OK  (已检测 {ok_count} 次, 失败 {fail_count} 次)")
            else:
                fail_count += 1
                if fail_count == 1:
                    first_fail_ts = ts
                # 计算累计断线时长
                duration = ""
                if fail_count == 1:
                    duration = " (首次断线)"
                else:
                    duration = f" (累计断线 {fail_count} 次)"

                print(f"[{ts}] 失败! {detail}{duration}")

                with open(log_path, "a", newline="", encoding="utf-8-sig") as f:
                    writer = csv.writer(f)
                    writer.writerow([ts, "FAIL", "", detail])

            time.sleep(args.interval)

    except KeyboardInterrupt:
        print()
        elapsed = fail_count * args.interval  # 粗略估算
        print(f"[{_timestamp()}] 监控已停止")
        print(
            f"[{_timestamp()}] 总计: 检测 {ok_count + fail_count} 次, "
            f"成功 {ok_count}, 失败 {fail_count}"
        )
        if first_fail_ts:
            print(f"[{_timestamp()}] 首次断线: {first_fail_ts}")
        print(f"[{_timestamp()}] 日志已保存: {log_path.resolve()}")
        return 0


if __name__ == "__main__":
    sys.exit(main())
