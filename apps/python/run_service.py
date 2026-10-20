from sysu_netauth.service.win_service import handle_command_line


def main() -> None:
    import sys

    handle_command_line(sys.argv)


if __name__ == "__main__":
    main()
