import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import openapiTS, { astToString } from 'openapi-typescript';

const spec = join(import.meta.dirname, '..', '..', 'docs-site', 'openapi.json');
const out = join(import.meta.dirname, '..', 'src', 'api', 'schema.d.ts');

const ast = await openapiTS(new URL(`file://${spec}`));
writeFileSync(out, astToString(ast));
