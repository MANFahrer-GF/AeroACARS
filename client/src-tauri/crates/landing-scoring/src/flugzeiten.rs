//! „Zeiten gegen Flugplan" (13.10.2026).
//!
//! Plan aus dem SimBrief-OFP (`<times>`: sched_out/off/on/in), Ist aus den
//! Zeitstempeln des Clients. Alle Werte Unix-Sekunden UTC. Reine Anzeige —
//! keine Note, kein Abzug liest das (Thomas: „kein Punktabzug, einfach nur
//! darstellen").
//!
//! Die Abweichungen rechnet die gespiegelte Anzeige (`lib/flugzeiten.ts`)
//! aus genau diesen acht Zahlen — Client und Webapp zeigen dasselbe.
//!
//! ⚠ Der SimBrief-Plan ist bei GSG meist kein echter Flugplan: `sched_out`
//! liegt fast immer rund 40 min nach dem Erstellen des OFP (30 PIREPs,
//! 60 Tage). Deshalb gibt es bewusst kein Urteil „puenktlich".

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FlugZeiten {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_off_block: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_abheben: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_aufsetzen: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_on_block: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub off_block: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abheben: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aufsetzen: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_block: Option<i64>,
}

impl FlugZeiten {
    /// Hat der Flug ueberhaupt einen Plan? Ohne Plan gibt es nichts zu
    /// vergleichen — die Anzeige sagt das dann ausdruecklich.
    pub fn hat_plan(&self) -> bool {
        self.plan_off_block.is_some()
            || self.plan_abheben.is_some()
            || self.plan_aufsetzen.is_some()
            || self.plan_on_block.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leere_felder_fehlen_im_json() {
        let z = FlugZeiten {
            plan_on_block: Some(100),
            on_block: Some(160),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_value(z).unwrap(),
            serde_json::json!({"plan_on_block": 100, "on_block": 160})
        );
        assert!(z.hat_plan());
        assert!(!FlugZeiten::default().hat_plan());
    }
}
