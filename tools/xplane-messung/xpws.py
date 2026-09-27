"""Kleiner WebSocket-Client für die X-Plane-Web-API v2 — nur Python-
Standardbibliothek (Michel soll nichts installieren müssen).

X-Plane schickt nach `dataref_subscribe_values` erst alle Werte und dann
laufend die geänderten (`dataref_update_values`). Ein Durchgang über ~8000
Werte dauert so Sekundenbruchteile statt 40 s wie bei Einzelabfragen.
"""

from __future__ import annotations

import base64
import json
import os
import socket
import struct
import threading


class XPlaneWS:
    def __init__(self, host: str = "127.0.0.1", port: int = 8086, pfad: str = "/api/v2"):
        self.sock = socket.create_connection((host, port), timeout=10)
        key = base64.b64encode(os.urandom(16)).decode()
        anfrage = (
            f"GET {pfad} HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\n"
            f"Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        )
        self.sock.sendall(anfrage.encode())
        kopf = b""
        while b"\r\n\r\n" not in kopf:
            teil = self.sock.recv(1024)
            if not teil:
                raise ConnectionError("WebSocket-Handshake abgebrochen")
            kopf += teil
        if b" 101 " not in kopf.split(b"\r\n", 1)[0]:
            raise ConnectionError("WebSocket abgelehnt: " + kopf.split(b"\r\n", 1)[0].decode("latin1"))
        self._rest = kopf.split(b"\r\n\r\n", 1)[1]
        self._req = 0
        self._lock = threading.Lock()
        self.sock.settimeout(None)

    # ── senden ──
    def _senden(self, opcode: int, daten: bytes) -> None:
        kopf = bytes([0x80 | opcode])
        n = len(daten)
        if n < 126:
            kopf += bytes([0x80 | n])
        elif n < 65536:
            kopf += bytes([0x80 | 126]) + struct.pack(">H", n)
        else:
            kopf += bytes([0x80 | 127]) + struct.pack(">Q", n)
        maske = os.urandom(4)
        maskiert = bytes(b ^ maske[i % 4] for i, b in enumerate(daten))
        with self._lock:
            self.sock.sendall(kopf + maske + maskiert)

    def senden_json(self, typ: str, params: dict) -> int:
        self._req += 1
        self._senden(1, json.dumps({"req_id": self._req, "type": typ, "params": params}).encode())
        return self._req

    # ── empfangen ──
    def _genau(self, n: int) -> bytes:
        while len(self._rest) < n:
            teil = self.sock.recv(max(65536, n - len(self._rest)))
            if not teil:
                raise ConnectionError("WebSocket geschlossen")
            self._rest += teil
        d, self._rest = self._rest[:n], self._rest[n:]
        return d

    def empfangen_json(self) -> dict | None:
        """Nächste vollständige Textnachricht als JSON; None bei Close."""
        teile = b""
        while True:
            b1, b2 = self._genau(2)
            fin, op = b1 & 0x80, b1 & 0x0F
            n = b2 & 0x7F
            if n == 126:
                n = struct.unpack(">H", self._genau(2))[0]
            elif n == 127:
                n = struct.unpack(">Q", self._genau(8))[0]
            if b2 & 0x80:
                maske = self._genau(4)
                roh = bytes(b ^ maske[i % 4] for i, b in enumerate(self._genau(n)))
            else:
                roh = self._genau(n)
            if op == 8:
                return None
            if op == 9:
                self._senden(10, roh)
                continue
            if op in (1, 2, 0):
                teile += roh
                if fin:
                    return json.loads(teile.decode("utf-8", "replace"))

    def schliessen(self) -> None:
        try:
            self._senden(8, b"")
            self.sock.close()
        except OSError:
            pass
