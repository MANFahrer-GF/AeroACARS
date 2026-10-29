// Texte `flugkarte.*` je Sprache, beim Bau eingelegt (vite.flugkarte.config.mjs).
declare module "virtual:flugkarte-texte" {
  const texte: Record<string, Record<string, unknown>>;
  export default texte;
}
