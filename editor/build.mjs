// Bundle the editor into the two files the riki binary embeds. Deterministic output: the
// committed bundle must equal a fresh build (otto `editor` diffs them).
import { build } from 'esbuild'

const common = {
  bundle: true,
  minify: true,
  legalComments: 'none',
  logLevel: 'warning',
  target: ['es2022'],
  charset: 'utf8',
  absWorkingDir: import.meta.dirname,
}

await build({
  ...common,
  entryPoints: ['src/main.ts'],
  outfile: '../server/assets/editor.js',
  format: 'iife',
  define: { 'process.env.NODE_ENV': '"production"', __VUE_OPTIONS_API__: 'false', __VUE_PROD_DEVTOOLS__: 'false', __VUE_PROD_HYDRATION_MISMATCH_DETAILS__: 'false' },
})

await build({
  ...common,
  entryPoints: ['src/editor.css'],
  outfile: '../server/assets/editor.css',
})
