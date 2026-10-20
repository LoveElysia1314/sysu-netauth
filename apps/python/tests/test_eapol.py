from __future__ import annotations

import hashlib
import unittest

from sysu_netauth.core.eapol import (
    EAP_IDENTITY,
    EAP_RESPONSE,
    build_identity_response,
    md5_challenge_response,
    parse_eapol,
)


class EapolCodecTests(unittest.TestCase):
    def test_identity_response_round_trip(self) -> None:
        packet = build_identity_response(
            "00:11:22:33:44:55",
            "01:80:c2:00:00:03",
            7,
            "netid",
        )

        parsed = parse_eapol(bytes(packet))

        self.assertIsNotNone(parsed)
        assert parsed is not None
        self.assertEqual(parsed.code, EAP_RESPONSE)
        self.assertEqual(parsed.identifier, 7)
        self.assertEqual(parsed.eap_type, EAP_IDENTITY)
        self.assertEqual(parsed.data, b"netid")

    def test_parser_rejects_truncated_declared_eap_length(self) -> None:
        packet = bytearray(
            bytes(
                build_identity_response(
                    "00:11:22:33:44:55",
                    "01:80:c2:00:00:03",
                    7,
                    "netid",
                )
            )
        )
        packet[20:22] = (255).to_bytes(2, "big")

        self.assertIsNone(parse_eapol(bytes(packet)))

    def test_md5_response_uses_gbk_password_bytes(self) -> None:
        challenge = bytes(range(16))
        expected = hashlib.md5(b"\x03" + "密码".encode("gbk") + challenge).digest()

        self.assertEqual(
            md5_challenge_response(3, "密码", challenge),
            expected,
        )


if __name__ == "__main__":
    unittest.main()
