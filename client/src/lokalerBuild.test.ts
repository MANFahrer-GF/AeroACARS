/** Der lokale Build darf die Signatur der CI nicht aushebeln.
 *
 *  Ein lokaler `tauri build` endete mit Fehlercode, obwohl die App fertig
 *  war: Tauri wollte die Updater-Dateien signieren, und der private
 *  Schluessel liegt bewusst nur in der CI. Die naheliegende "Loesung" waere,
 *  `createUpdaterArtifacts` global abzuschalten — dann baute die CI keine
 *  `.sig` und keine `latest.json` mehr, und das Auto-Update aller Piloten
 *  stuende still, ohne dass es jemand merkt.
 *
 *  Deshalb: Hauptkonfiguration bleibt an, nur die lokale Beikonfiguration
 *  schaltet ab (`npm run build:lokal`).
 */
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";

const lies = (p: string) => JSON.parse(readFileSync(p, "utf8"));

describe("Updater-Signatur", () => {
  it("bleibt in der Hauptkonfiguration eingeschaltet", () => {
    const c = lies("src-tauri/tauri.conf.json");
    expect(c.bundle.createUpdaterArtifacts).toBe(true);
  });

  it("ist im Client ein oeffentlicher Schluessel hinterlegt", () => {
    const c = lies("src-tauri/tauri.conf.json");
    expect(c.plugins?.updater?.pubkey?.length ?? 0).toBeGreaterThan(100);
  });

  it("schaltet nur die lokale Beikonfiguration ab", () => {
    const l = lies("src-tauri/tauri.lokal.conf.json");
    expect(l.bundle.createUpdaterArtifacts).toBe(false);
    // Sie darf sonst nur die Kennung umstellen — sonst weicht der lokale Build
    // still vom Release ab und man testet etwas anderes als man ausliefert.
    expect(Object.keys(l.bundle)).toEqual(["createUpdaterArtifacts"]);
    expect(Object.keys(l).filter((k) => k !== "$schema").sort()).toEqual(["bundle", "identifier", "productName"]);
  });

  it("nutzt die Beikonfiguration nur in den lokalen Skripten", () => {
    const pkg = lies("package.json");
    expect(pkg.scripts["build:lokal"]).toContain("tauri.lokal.conf.json");
    expect(pkg.scripts["dev:app"]).toContain("tauri.lokal.conf.json");
    // Kein anderes Skript darf sie ziehen, damit ein Release-Build sie nie sieht.
    const andere = Object.entries(pkg.scripts as Record<string, string>)
      .filter(([k, v]) => k !== "build:lokal" && k !== "dev:app" && v.includes("tauri.lokal"));
    expect(andere).toEqual([]);
  });
});

/** P-11.21 (B-159): Ein Dev-Build hat eine eigene Kennung und damit einen
 *  eigenen Datenordner — ein Probestart beruehrt nie Sitzung oder Flug der
 *  Beta (`com.aeroacars.app3`). Der Rust-Teil (Log-Ordner, Datenuebernahme)
 *  folgt derselben Kennung ueber build.rs (`AEROACARS_KENNUNG`). */
describe("Dev-Kennung (P-11.21)", () => {
  it("unterscheidet sich von der Beta in Kennung und Name", () => {
    const c = lies("src-tauri/tauri.conf.json");
    const l = lies("src-tauri/tauri.lokal.conf.json");
    expect(c.identifier).toBe("com.aeroacars.app3");
    expect(l.identifier).toBe("com.aeroacars.app3.dev");
    expect(l.productName).not.toBe(c.productName);
  });

  it("gilt fuer beide Laeufer des Dev-Builds, nie fuer das Release", () => {
    const dev = readFileSync("../.github/workflows/dev-build.yml", "utf8");
    const bauen = dev.split("\n").filter((z) => z.includes("tauri build"));
    expect(bauen.length).toBe(2);
    // Ohne den Eingang `beta` die Dev-Kennung; nur mit `beta` die
    // Produktionskennung (Beta-Installer mit eigenem Update-Kanal).
    const wahl = "--config src-tauri/${{ inputs.beta && 'tauri.beta.conf.json' || 'tauri.lokal.conf.json' }}";
    for (const z of bauen) expect(z).toContain(wahl);
    const beta = lies("src-tauri/tauri.beta.conf.json");
    expect(beta.identifier).toBeUndefined();
    expect(readFileSync("../.github/workflows/release.yml", "utf8")).not.toContain("tauri.lokal");
  });
});

/** Beta-Update-Kanal: Beta-Installer aktualisieren sich selbst, aber nur aus
 *  dem eigenen Kanal. Fragten sie den 2.x-Kanal (releases/latest), faenden sie
 *  2.1.x < 3.0.0-beta und blieben stehen; landete umgekehrt eine Beta im
 *  2.x-Kanal, bekaeme jeder 2.x-Pilot sie angeboten. */
describe("Beta-Update-Kanal", () => {
  const KANAL = "https://github.com/MANFahrer-GF/AeroACARS/releases/download/aeroacars3-beta-kanal/latest.json";

  it("schaltet in der Beta-Konfiguration die Updater-Dateien ein, mit eigenem Endpunkt", () => {
    const beta = lies("src-tauri/tauri.beta.conf.json");
    expect(beta.bundle.createUpdaterArtifacts).toBe(true);
    // Merge-Patch ersetzt Arrays: genau dieser eine Endpunkt, kein 2.x-Kanal davor.
    expect(beta.plugins.updater.endpoints).toEqual([KANAL]);
    // Schluessel kommt aus der Hauptkonfiguration — derselbe wie 2.x.
    expect(beta.plugins.updater.pubkey).toBeUndefined();
    expect(Object.keys(beta.plugins.updater)).toEqual(["endpoints"]);
    expect(Object.keys(beta).filter((k) => k !== "$schema").sort()).toEqual(["bundle", "plugins"]);
    expect(Object.keys(beta.bundle)).toEqual(["createUpdaterArtifacts"]);
  });

  it("gibt auch dem finalen 3.x einen eigenen Kanal, nie releases/latest (2.x)", () => {
    // 3.x ist GSG-exklusiv mit eigener Kennung: es darf weder 2.x-Updates
    // ziehen noch ueber releases/latest an 2.x-Installationen gehen.
    const c = lies("src-tauri/tauri.conf.json");
    expect(c.plugins.updater.endpoints).toEqual([
      "https://github.com/MANFahrer-GF/AeroACARS/releases/download/aeroacars3-kanal/latest.json",
    ]);
    expect(JSON.stringify(c)).not.toContain("releases/latest");
    expect(c.plugins.updater.endpoints).not.toEqual(lies("src-tauri/tauri.beta.conf.json").plugins.updater.endpoints);
    expect(readFileSync("../.github/workflows/release.yml", "utf8")).not.toContain("tauri.beta");
  });

  it("baut v3-Releases als Pre-Release und gibt sie nur ohne latest frei", () => {
    const rel = readFileSync("../.github/workflows/release.yml", "utf8");
    const zeile = rel.split("\n").find((z) => z.trim().startsWith("prerelease:"));
    expect(zeile).toContain("startsWith(github.ref_name, 'v3.')");
    const skript = readFileSync("../scripts/beta-veroeffentlichen.sh", "utf8");
    expect(skript).toContain('gh release edit "$TAG" -R "$REPO" --draft=false --prerelease=false --latest=false');
    expect(skript).toContain("KANAL_FINAL=aeroacars3-kanal");
    // Nach jedem Schritt pruefen, dass GitHub-latest ein 2.x bleibt.
    expect(skript.match(/^\s*pruefe_latest_2x$/gm)?.length ?? 0).toBeGreaterThanOrEqual(4);
  });

  it("verlangt im Beta-Build die Plugin-Pruefsumme und reicht sie in die App", () => {
    const dev = readFileSync("../.github/workflows/dev-build.yml", "utf8");
    const env = dev.split("\n").filter((z) => z.includes("AEROACARS_XPLANE_PLUGIN_SHA256:"));
    expect(env.length).toBe(2);
    for (const z of env) expect(z).toContain("needs.plugin-zip.outputs.sha256");
    expect(dev.match(/name: Require X-Plane plugin SHA-256 \(beta\)/g)?.length).toBe(2);
    expect(dev).toContain('ZIP="AeroACARS-XPlane-Plugin-v${VER}.zip"');
    expect(dev).toContain('beta-kanal-bauen.sh roh kanal "$PLUGIN_SHA256"');
    // Die App laedt das Paket unter genau diesem Namen.
    const rs = readFileSync("src-tauri/src/xplane_plugin_install.rs", "utf8");
    expect(rs).toContain("releases/download/v{VERSION}/AeroACARS-XPlane-Plugin-v{VERSION}.zip");
    const skript = readFileSync("../scripts/beta-veroeffentlichen.sh", "utf8");
    expect(skript).toContain('"AeroACARS-XPlane-Plugin-${TAG}.zip"');
  });

  it("signiert im Dev-Build nur bei beta und baut das Kanal-Paket, ohne zu veroeffentlichen", () => {
    const dev = readFileSync("../.github/workflows/dev-build.yml", "utf8");
    const schluessel = dev.split("\n").filter((z) => z.includes("TAURI_SIGNING_PRIVATE_KEY:"));
    expect(schluessel.length).toBe(2);
    for (const z of schluessel) expect(z).toContain("inputs.beta && secrets.TAURI_SIGNING_PRIVATE_KEY || ''");
    expect(dev).toContain(".github/scripts/beta-kanal-bauen.sh");
    expect(dev).not.toMatch(/gh release|softprops|tauri-action/);
    // Mac-Beta immer Apple-signiert: das Update ersetzt die App der Piloten.
    expect(dev).toContain("if: inputs.apple_signieren || inputs.beta");
  });

  it("schliesst v3-Beta-Tags vom Release-Build aus", () => {
    const rel = readFileSync("../.github/workflows/release.yml", "utf8");
    expect(rel).toContain("- '!v3.*-beta*'");
    // Das Skript legt diese Tags an und prueft den Ausschluss am Commit.
    const skript = readFileSync("../scripts/beta-veroeffentlichen.sh", "utf8");
    expect(skript).toContain(String.raw`'!v3\.\*-beta\*'`);
    expect(skript).toContain("KANAL=aeroacars3-beta-kanal");
    expect(KANAL).toContain("/aeroacars3-beta-kanal/");
    // Kein v-Praefix am Kanal-Tag: release.yml laeuft auf v*.
    // Jeder ausgefuehrte gh-release-Befehl setzt latest nur auf false (der
    // Rueckstell-Hinweis im Fehlertext ist kein Befehl).
    const befehle = skript.split("\n").filter((z) => /^\s*gh release (create|edit)/.test(z));
    expect(befehle.length).toBe(3);
    for (const z of befehle) {
      expect(z).toContain("--latest=false");
      expect(z).not.toMatch(/--latest(?!=false)/);
    }
  });

  it("nutzt beim Bauen und Veroeffentlichen dieselben Dateinamen", () => {
    // beta-kanal-bauen.sh benennt nach productName (Leerzeichen -> Punkt),
    // beta-veroeffentlichen.sh erwartet die Namen fest. Ein Umbenennen der
    // App fiele sonst erst beim Veroeffentlichen auf.
    const c = lies("src-tauri/tauri.conf.json");
    const bauen = readFileSync("../.github/scripts/beta-kanal-bauen.sh", "utf8");
    const skript = readFileSync("../scripts/beta-veroeffentlichen.sh", "utf8");
    expect(bauen).toContain('P="${NAME// /.}_${VER}"');
    expect(skript).toContain(`P="${c.productName.replace(/ /g, ".")}_\${VER}"`);
    for (const endung of ["_x64-setup.exe", "_x64-setup.exe.sig", "_aarch64.dmg", "_aarch64.app.tar.gz", "_aarch64.app.tar.gz.sig"]) {
      expect(bauen).toContain(`"$ZIEL/\${P}${endung}"`);
      expect(skript).toContain(`"\${P}${endung}"`);
    }
  });
});
