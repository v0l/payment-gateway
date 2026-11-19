import { readFileSync } from 'node:fs';
import { join } from 'node:path';

interface Operation {
  summary: string;
  'x-required-scope'?: string;
  parameters?: { name: string; in: string; required?: boolean }[];
  requestBody?: { content: { 'application/json': { schema: { $ref: string } } } };
}

interface Spec {
  info: { title: string; version: string };
  paths: Record<string, Record<string, Operation>>;
  components: { schemas: Record<string, { properties?: Record<string, { type?: string }> }> };
}

export function referenceMarkdown(docsDir: string): string {
  const spec = JSON.parse(readFileSync(join(docsDir, 'openapi.json'), 'utf8')) as Spec;
  const lines = [
    '# API reference',
    '',
    `${spec.info.title}, version ${spec.info.version}. Generated from the OpenAPI document served at \`/v1/openapi.json\`. Authenticate with an \`Authorization: Bearer\` header carrying a project API key. List endpoints accept \`limit\` (1 to 100) and \`cursor\`, and return the next cursor in the \`X-Next-Cursor\` header.`,
    '',
  ];
  for (const path of Object.keys(spec.paths)) {
    for (const method of Object.keys(spec.paths[path])) {
      const op = spec.paths[path][method];
      lines.push(`## ${method.toUpperCase()} ${path}`, '', `${op.summary}. Required scope: \`${op['x-required-scope']}\`.`, '');
      const params = op.parameters ?? [];
      if (params.length) {
        lines.push('| Parameter | In | Required |', '| --- | --- | --- |');
        for (const p of params) lines.push(`| \`${p.name}\` | ${p.in} | ${p.required ? 'yes' : 'no'} |`);
        lines.push('');
      }
      if (op.requestBody) {
        const ref = op.requestBody.content['application/json'].schema.$ref.split('/').pop()!;
        const schema = spec.components.schemas[ref];
        lines.push(`Request body \`${ref}\`:`, '');
        const props = Object.keys(schema?.properties ?? {});
        if (props.length) {
          lines.push('| Field | Type |', '| --- | --- |');
          for (const k of props) lines.push(`| \`${k}\` | ${schema.properties![k].type || 'any'} |`);
          lines.push('');
        }
      }
    }
  }
  return `${lines.join('\n')}\n`;
}
