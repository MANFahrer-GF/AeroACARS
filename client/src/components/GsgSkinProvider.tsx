// Skin fuer den Pilot-Client: ueber Rust mit dem Pilot-Token (29.09.2026).
//
// `SkinContext.tsx` ist mit der Webapp byteweise gleich und kennt deshalb
// weder `invoke` noch das Token. Hier wird ihm der Client-Ladeweg
// uebergeben — und der Takt, der nach der Anmeldung neu laden laesst.

import type { ReactNode } from "react";
import { liveInhalt, useLiveZugangTakt } from "../lib/liveInhalt";
import { SkinProvider, type SkinLaden } from "./SkinContext";
import type { V2Skin } from "./runwayV2Skin";

const skinUeberLive: SkinLaden = (signal) =>
  liveInhalt<Partial<V2Skin>>("/api/v2-skin", signal);

export function GsgSkinProvider({ children }: { children: ReactNode }) {
  const takt = useLiveZugangTakt();
  return (
    <SkinProvider laden={skinUeberLive} neuLaden={takt}>
      {children}
    </SkinProvider>
  );
}
