import commonjs from "@rollup/plugin-commonjs";
import nodeResolve from "@rollup/plugin-node-resolve";

const sdPlugin = "eu.german-sky-group.aeroacars.sdPlugin";

export default {
  input: "src/plugin.js",
  output: { file: `${sdPlugin}/bin/plugin.js`, format: "es" },
  plugins: [
    nodeResolve({ browser: false, exportConditions: ["node"], preferBuiltins: true }),
    commonjs(),
    {
      name: "modul-paket",
      generateBundle() {
        this.emitFile({ fileName: "package.json", source: `{ "type": "module" }`, type: "asset" });
      },
    },
  ],
};
