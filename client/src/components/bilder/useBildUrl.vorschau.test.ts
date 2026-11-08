import { beforeEach, describe, expect, it, vi } from "vitest";

const h = vi.hoisted(() => ({ lesen: vi.fn(async (_p: string) => new Uint8Array([1, 2])) }));
vi.mock("./logik/api", () => ({ bildAktionen: { lesen: h.lesen } }));
import { vorschauAdresse } from "./useBildUrl";
import { eintrag } from "./logik/testhilfen";

describe("vorschauAdresse", () => {
  beforeEach(() => {
    h.lesen.mockClear();
    vi.stubGlobal("URL", Object.assign(URL, { createObjectURL: () => "blob:klein", revokeObjectURL: () => undefined }));
  });
  it("laedt die kleine Datei", async () => {
    expect(await vorschauAdresse({ ...eintrag({ id: "a" }), pfad: "g/a.jpg", vorschau_pfad: "g/a.vorschau.jpg" })).toBe("blob:klein");
    expect(h.lesen).toHaveBeenCalledWith("g/a.vorschau.jpg");
  });
  it("ohne kleine Datei wird das Vollbild nicht angefasst", async () => {
    expect(await vorschauAdresse({ ...eintrag({ id: "b" }), pfad: "g/b.jpg", vorschau_pfad: null })).toBeNull();
    expect(h.lesen).not.toHaveBeenCalled();
  });
  it("Lesefehler und fehlender Eintrag ergeben null", async () => {
    expect(await vorschauAdresse(undefined)).toBeNull();
    h.lesen.mockRejectedValueOnce(new Error("weg"));
    expect(await vorschauAdresse({ ...eintrag({ id: "c" }), pfad: null, vorschau_pfad: "g/c.fehlt.jpg" })).toBeNull();
  });
});
