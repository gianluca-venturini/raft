const esbuild = require('esbuild');
const path = require('path');
const { wasmLoader } = require('esbuild-plugin-wasm');

esbuild.build({
    entryPoints: [path.join(__dirname, 'src/index.ts')],
    bundle: true,
    outfile: path.join(__dirname, 'dist/bundle.js'),
    format: 'esm',
    target: ['esnext'],
    loader: {
        '.ts': 'ts',
    },
    plugins: [wasmLoader({
        mode: 'embedded',
    })]
}).catch(() => process.exit(1));