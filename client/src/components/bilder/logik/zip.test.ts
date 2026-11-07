import { describe, expect, it } from "vitest";
import { crc32, zipBauen, zipName } from "./zip";

describe("ZIP-Export", () => {
  it("CRC-32 stimmt mit dem bekannten Wert fuer „123456789“ ueberein", () => {
    expect(crc32(new TextEncoder().encode("123456789"))).toBe(0xcbf43926);
  });
  it("Aufbau: Kopfkennung, Dateizahl, Inhalte und Namen lesbar zurueck", () => {
    const zip = zipBauen([
      { name: "a.jpg", daten: Uint8Array.from([1, 2, 3]) },
      { name: "b.json", daten: new TextEncoder().encode("{}") },
    ]);
    const v = new DataView(zip.buffer);
    expect(v.getUint32(0, true)).toBe(0x04034b50);
    const ende = zip.length - 22;
    expect(v.getUint32(ende, true)).toBe(0x06054b50);
    expect(v.getUint16(ende + 10, true)).toBe(2);
    const zentralStart = v.getUint32(ende + 16, true);
    expect(v.getUint32(zentralStart, true)).toBe(0x02014b50);
    expect(v.getUint32(zentralStart + 16, true)).toBe(crc32(Uint8Array.from([1, 2, 3])));
    // erste Datei: Name hinter dem 30-Byte-Kopf, danach die Daten
    expect(new TextDecoder().decode(zip.subarray(30, 35))).toBe("a.jpg");
    expect(Array.from(zip.subarray(35, 38))).toEqual([1, 2, 3]);
  });
  it("gleiche Namen werden durchnummeriert, Pfade entschaerft", () => {
    expect(zipName("../../etc/passwd")).not.toMatch(/[\\/]/);
    expect(zipName("../../etc/passwd").startsWith(".")).toBe(false);
    expect(zipName("../x")).not.toContain("/");
    expect(zipName("")).toBe("datei");
    const zip = zipBauen([{ name: "a.jpg", daten: new Uint8Array() }, { name: "a.jpg", daten: new Uint8Array() }]);
    const text = new TextDecoder().decode(zip);
    expect(text).toContain("a.jpg");
    expect(text).toContain("a-2.jpg");
  });
});
