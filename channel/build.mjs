// 의존성을 모두 넣은 단일 파일 dist/gyeot-channel.mjs를 만든다(설치 = 이 파일 하나 복사).
// 묶인 패키지의 라이선스 글은 파일 끝 주석으로 붙인다(MIT 등은 사본에 고지를 요구한다).
import { build } from 'esbuild';
import { appendFile, readFile, readdir } from 'node:fs/promises';
import path from 'node:path';

const outfile = 'dist/gyeot-channel.mjs';

const result = await build({
  entryPoints: ['src/main.ts'],
  outfile,
  bundle: true,
  platform: 'node',
  format: 'esm',
  target: 'node20',
  // 묶인 CommonJS 의존성(ajv 등)이 require를 쓸 수 있게 한다.
  banner: {
    js: "import { createRequire as __gyeotCreateRequire } from 'node:module';\nconst require = __gyeotCreateRequire(import.meta.url);",
  },
  legalComments: 'eof',
  metafile: true,
  sourcemap: false,
  minify: false,
  logLevel: 'warning',
});

const packages = new Set();
for (const input of Object.keys(result.metafile.inputs)) {
  const m = /(?:^|\/)node_modules\/((?:@[^/]+\/)?[^/]+)\//.exec(input);
  if (m) packages.add(m[1]);
}

let notice = '\n/*\n * 묶인 제3자 패키지와 라이선스 (third-party notices)\n';
for (const name of [...packages].sort()) {
  const dir = path.join('node_modules', name);
  const pkg = JSON.parse(await readFile(path.join(dir, 'package.json'), 'utf8'));
  notice += ` *\n * ---- ${name}@${pkg.version} (${pkg.license ?? '라이선스 표기 없음'})\n`;
  const file = (await readdir(dir)).find((f) => /^(licen[cs]e|copying)(\..*)?$/i.test(f));
  if (file) {
    const text = (await readFile(path.join(dir, file), 'utf8')).replaceAll('*/', '* /').trimEnd();
    notice += text
      .split(/\r?\n/)
      .map((l) => ` * ${l}`.trimEnd())
      .join('\n');
    notice += '\n';
  }
}
notice += ' */\n';
await appendFile(outfile, notice);
