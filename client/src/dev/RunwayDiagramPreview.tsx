// Dev-only Preview-Tab für RunwayDiagramV2.
//
// Sichtbar nur in `npm run tauri dev` (import.meta.env.DEV). Im
// Production-Build (`vite build`) blendet die App-Tab-Bar diesen Tab
// aus. Bewusster Workflow:
//
//   1. cd client && npm run tauri dev
//   2. Tab "🧪 Preview" oben anklicken
//   3. Mock-Variante im Picker auswählen (14 Varianten, davon 10 die
//      Pflichtvarianten aus docs/spec/v1.7.0-bahndisziplin.md §11)
//   4. Container-Breite mit Slider verändern → Responsive testen
//   5. Edits in RunwayDiagramV2.tsx oder RunwayGlossaryModal.tsx →
//      Vite Hot-Reload, KEIN Tauri-Rebuild
//
// Spec: docs/spec/runway-diagram-v2.contract.md
//       docs/spec/v1.7.0-bahndisziplin.md §11 -- die Demo ist dort KEIN
//       Nebenprodukt, sondern die Abnahmestufe, an der die Bandgrenzen aus
//       §4 und §5.4 entschieden werden. Am Schreibtisch laesst sich nicht
//       sinnvoll festlegen, was 3 m Randabstand gegenueber 15 m kosten soll.

import { useMemo, useState } from "react";
import { RunwayDiagramV2 } from "../components/RunwayDiagramV2";
import { mapLandingRecordToV2Props } from "./runwayDiagramV2Mapper";
import type { LandingRecord, LandingRunwayMatch } from "../components/LandingPanel";
import { MOCK_LANDING_OPTIONS, type MockKey } from "./mockLandingRecords";

export default function RunwayDiagramPreview() {
  const [key, setKey] = useState<MockKey>("ms713");
  const [containerWidth, setContainerWidth] = useState<number>(1100);
  const [showJson, setShowJson] = useState(false);

  const opt = useMemo(
    () => MOCK_LANDING_OPTIONS.find((o) => o.key === key) ?? MOCK_LANDING_OPTIONS[0]!,
    [key],
  );
  const record: LandingRecord = useMemo(() => opt.build(), [opt]);
  const rw = record.runway_match as LandingRunwayMatch;
  const v2Props = useMemo(() => mapLandingRecordToV2Props(record), [record]);

  return (
    <section className="phase" style={{ display: "flex", flexDirection: "column", gap: 16 }}>
      <header
        style={{
          display: "flex",
          flexDirection: "column",
          gap: 8,
          padding: 12,
          background: "rgba(34,197,94,0.08)",
          border: "1px solid rgba(34,197,94,0.35)",
          borderRadius: 6,
        }}
      >
        <strong style={{ color: "#22c55e" }}>
          🧪 Dev-Preview RunwayDiagramV2 — nur in `npm run tauri dev` sichtbar
        </strong>
        <span style={{ fontSize: "0.85rem", opacity: 0.85 }}>
          Synthetische LandingRecords zum Design-Iterieren der neuen V2-
          Geometrie. Edits in{" "}
          <code>src/components/RunwayDiagramV2.tsx</code> oder{" "}
          <code>RunwayGlossaryModal.tsx</code> triggern Hot-Reload. Live-
          LandingPanel bleibt unangetastet (kein Pilot-Release).
        </span>
      </header>

      <div
        style={{
          display: "flex",
          gap: 16,
          alignItems: "center",
          flexWrap: "wrap",
          padding: 10,
          background: "rgba(255,255,255,0.04)",
          borderRadius: 4,
        }}
      >
        <label style={{ display: "flex", gap: 6, alignItems: "center" }}>
          <span style={{ fontWeight: 600 }}>Variante:</span>
          <select
            value={key}
            onChange={(e) => setKey(e.target.value as MockKey)}
            style={{ padding: "4px 8px", minWidth: 360 }}
          >
            {MOCK_LANDING_OPTIONS.map((o) => (
              <option key={o.key} value={o.key}>
                {o.label}
              </option>
            ))}
          </select>
        </label>
        <label style={{ display: "flex", gap: 6, alignItems: "center" }}>
          <span style={{ fontWeight: 600 }}>Container:</span>
          <input
            type="range"
            min={420}
            max={1400}
            step={20}
            value={containerWidth}
            onChange={(e) => setContainerWidth(Number(e.target.value))}
          />
          <span style={{ fontFamily: "monospace", minWidth: 60 }}>
            {containerWidth} px
          </span>
        </label>
        <button type="button" onClick={() => setShowJson((v) => !v)}>
          {showJson ? "Hide raw JSON" : "Show raw JSON"}
        </button>
      </div>

      <p style={{ fontSize: "0.85rem", opacity: 0.7, margin: 0 }}>
        💡 {opt.hint}
      </p>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(140px, 1fr))",
          gap: 8,
          fontSize: "0.85rem",
        }}
      >
        <Stat label="Score" value={`${record.score_numeric} (${record.grade_letter})`} />
        <Stat label="Bahn" value={`${rw.airport_ident}/${rw.runway_ident}`} />
        <Stat label="Länge" value={`${(rw.length_ft * 0.3048).toFixed(0)} m`} />
        <Stat
          label="Centerline"
          value={`${rw.centerline_distance_m.toFixed(1)} m ${rw.side}`}
        />
        <Stat
          label="Past threshold"
          value={`${(rw.touchdown_distance_from_threshold_ft * 0.3048).toFixed(0)} m`}
        />
        <Stat
          label="TDZ"
          value={
            record.td_in_tdz == null
              ? "n/a"
              : record.td_in_tdz
              ? `✓ third ${record.td_third}`
              : `✗ third ${record.td_third}`
          }
        />
        <Stat
          label="Aim Δ"
          value={
            record.aim_delta_m != null
              ? `${record.aim_delta_m >= 0 ? "+" : ""}${record.aim_delta_m.toFixed(0)} m · ${record.aim_class}`
              : "—"
          }
        />
        <Stat
          label="TCH"
          value={
            record.tch_actual_ft != null
              ? `${record.tch_actual_ft.toFixed(0)} ft (Δ ${record.tch_delta_ft?.toFixed(0)})`
              : "—"
          }
        />
        <Stat
          label="Source"
          value={rw.source ?? "—"}
        />
      </div>

      <div
        style={{
          width: containerWidth,
          maxWidth: "100%",
          border: "1px dashed rgba(255,255,255,0.2)",
          padding: 6,
          background: "rgba(0,0,0,0.2)",
        }}
      >
        {v2Props ? (
          <RunwayDiagramV2 {...v2Props} />
        ) : (
          <div style={{ padding: 24, opacity: 0.7, fontStyle: "italic" }}>
            Kein runway_match — V2 rendert nicht.
          </div>
        )}
      </div>

      {showJson && (
        <pre
          style={{
            fontSize: "0.72rem",
            background: "rgba(0,0,0,0.3)",
            padding: 10,
            borderRadius: 4,
            maxHeight: 320,
            overflow: "auto",
            margin: 0,
          }}
        >
          {JSON.stringify({ record, v2Props }, null, 2)}
        </pre>
      )}
    </section>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div
      style={{
        padding: "6px 10px",
        background: "rgba(255,255,255,0.05)",
        borderRadius: 4,
        border: "1px solid rgba(255,255,255,0.08)",
      }}
    >
      <div style={{ opacity: 0.6, fontSize: "0.72rem", textTransform: "uppercase" }}>
        {label}
      </div>
      <div style={{ fontWeight: 600 }}>{value}</div>
    </div>
  );
}
