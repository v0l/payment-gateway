import { readFileSync, writeFileSync, readdirSync, mkdirSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(fileURLToPath(import.meta.url));
const out = join(root, "..", "web", "site");
const nav = JSON.parse(readFileSync(join(root, "nav.json"), "utf8"));
const check = process.argv.includes("--check");

const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

function inline(text) {
  const codes = [];
  let s = text.replace(/`([^`]+)`/g, (_, c) => {
    codes.push(`<code>${esc(c)}</code>`);
    return `\u0000${codes.length - 1}\u0000`;
  });
  s = esc(s);
  s = s.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  s = s.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (_, label, href) => `<a href="${href}">${label}</a>`);
  return s.replace(/\u0000(\d+)\u0000/g, (_, i) => codes[Number(i)]);
}

function expandIncludes(md) {
  return md.replace(/^@include (\S+) (\S+)$/gm, (_, file, lang) => {
    const body = readFileSync(join(root, file), "utf8").replace(/\n$/, "");
    return "```" + lang + " sample\n" + body + "\n```";
  });
}

function cells(line) {
  return line.trim().replace(/^\|/, "").replace(/\|$/, "").split("|").map((c) => c.trim());
}

function referenceMarkdown() {
  const spec = JSON.parse(readFileSync(join(root, "openapi.json"), "utf8"));
  const lines = [`# API reference`, "", `${spec.info.title}, version ${spec.info.version}. Generated from the OpenAPI document served at \`/v1/openapi.json\`. Authenticate with an \`Authorization: Bearer\` header carrying a project API key. List endpoints accept \`limit\` (1 to 100) and \`cursor\`, and return the next cursor in the \`X-Next-Cursor\` header.`, ""];
  for (const path of Object.keys(spec.paths)) {
    for (const method of Object.keys(spec.paths[path])) {
      const op = spec.paths[path][method];
      lines.push(`## ${method.toUpperCase()} ${path}`, "", `${op.summary}. Required scope: \`${op["x-required-scope"]}\`.`, "");
      const params = op.parameters || [];
      if (params.length) {
        lines.push("| Parameter | In | Required |", "| --- | --- | --- |");
        for (const p of params) lines.push(`| \`${p.name}\` | ${p.in} | ${p.required ? "yes" : "no"} |`);
        lines.push("");
      }
      if (op.requestBody) {
        const ref = op.requestBody.content["application/json"].schema.$ref.split("/").pop();
        const schema = spec.components.schemas[ref];
        lines.push(`Request body \`${ref}\`:`, "");
        const props = Object.keys((schema && schema.properties) || {});
        if (props.length) {
          lines.push("| Field | Type |", "| --- | --- |");
          for (const k of props) lines.push(`| \`${k}\` | ${schema.properties[k].type || "any"} |`);
          lines.push("");
        }
      }
    }
  }
  return lines.join("\n") + "\n";
}

function render(md) {
  const lines = expandIncludes(md).split("\n");
  const html = [];
  let i = 0;
  const headings = [];
  while (i < lines.length) {
    const line = lines[i];
    if (line.startsWith("```")) {
      const info = line.slice(3).trim().split(/\s+/);
      const buf = [];
      i++;
      while (i < lines.length && !lines[i].startsWith("```")) buf.push(lines[i++]);
      i++;
      html.push(`<pre><code class="language-${esc(info[0] || "text")}">${esc(buf.join("\n"))}</code></pre>`);
      continue;
    }
    const h = /^(#{1,3}) (.+)$/.exec(line);
    if (h) {
      const id = h[2].toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
      headings.push([h[1].length, h[2], id]);
      html.push(`<h${h[1].length} id="${id}">${inline(h[2])}</h${h[1].length}>`);
      i++;
      continue;
    }
    if (line.startsWith("|") && /^\|[\s:|-]+\|$/.test(lines[i + 1] || "")) {
      const head = cells(line);
      i += 2;
      const rows = [];
      while (i < lines.length && lines[i].startsWith("|")) rows.push(cells(lines[i++]));
      html.push(
        "<table><thead><tr>" + head.map((c) => `<th scope="col">${inline(c)}</th>`).join("") + "</tr></thead><tbody>" +
          rows.map((r) => "<tr>" + r.map((c) => `<td>${inline(c)}</td>`).join("") + "</tr>").join("") + "</tbody></table>",
      );
      continue;
    }
    const li = /^(-|\d+\.) (.*)$/.exec(line);
    if (li) {
      const tag = li[1] === "-" ? "ul" : "ol";
      const items = [];
      while (i < lines.length) {
        const m = /^(-|\d+\.) (.*)$/.exec(lines[i]);
        if (!m) break;
        let text = m[2];
        i++;
        while (i < lines.length && /^  \S/.test(lines[i])) text += " " + lines[i++].trim();
        items.push(`<li>${inline(text)}</li>`);
      }
      html.push(`<${tag}>${items.join("")}</${tag}>`);
      continue;
    }
    if (line.startsWith("> ")) {
      const buf = [];
      while (i < lines.length && lines[i].startsWith("> ")) buf.push(lines[i++].slice(2));
      html.push(`<blockquote><p>${inline(buf.join(" "))}</p></blockquote>`);
      continue;
    }
    if (line.trim() === "") {
      i++;
      continue;
    }
    const buf = [];
    while (i < lines.length && lines[i].trim() !== "" && !/^(```|#{1,3} |\||> |- |\d+\. )/.test(lines[i])) buf.push(lines[i++]);
    if (!buf.length) buf.push(lines[i++]);
    html.push(`<p>${inline(buf.join(" "))}</p>`);
  }
  return { html: html.join("\n"), headings };
}

export function page({ title, body, section, slug, banner }) {
  const links = nav.sections
    .map((s) => {
      const items = s.pages
        .map((p) => {
          const href = p.slug === "index" ? `/${s.id}/` : `/${s.id}/${p.slug}`;
          const current = s.id === section && p.slug === slug ? ' aria-current="page"' : "";
          return `<li><a href="${href}"${current}>${esc(p.title)}</a></li>`;
        })
        .join("");
      return `<h2>${esc(s.title)}</h2><ul>${items}</ul>`;
    })
    .join("");
  const note = banner ? `<p class="draft" role="note">${esc(banner)}</p>` : "";
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${esc(title)} | Payment Gateway</title>
<link rel="stylesheet" href="/assets/tokens.css">
<link rel="stylesheet" href="/assets/site.css">
</head>
<body>
<a class="skip" href="#content">Skip to content</a>
<header class="top">
<a class="brand" href="/">Payment Gateway</a>
<nav aria-label="Primary"><a href="/guide/">Docs</a> <a href="/operator/">Operators</a> <a href="/docs">API reference</a> <a href="/legal/">Legal</a> <a class="button" href="/console">Sign in</a></nav>
</header>
<div class="layout">
<nav class="side" aria-label="Documentation">${links}</nav>
<main id="content">
${note}
${body}
</main>
</div>
</body>
</html>
`;
}

function write(path, content, problems) {
  if (check) {
    const current = existsSync(path) ? readFileSync(path, "utf8") : "";
    if (current !== content) problems.push(path);
    return;
  }
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, content);
}

const problems = [];
const known = new Set();
for (const section of nav.sections) {
  for (const p of section.pages) {
    known.add(`${section.id}/${p.slug}.md`);
    const md = p.slug === "reference" && section.id === "guide" ? referenceMarkdown() : readFileSync(join(root, section.id, `${p.slug}.md`), "utf8");
    const { html } = render(md);
    const body = `<article>\n${html}\n</article>`;
    write(
      join(out, `${section.id}-${p.slug}.html`),
      page({ title: p.title, body, section: section.id, slug: p.slug, banner: section.banner }),
      problems,
    );
  }
}
for (const section of nav.sections) {
  if (!existsSync(join(root, section.id))) continue;
  for (const f of readdirSync(join(root, section.id))) {
    if (f.endsWith(".md") && !known.has(`${section.id}/${f}`)) problems.push(`${section.id}/${f} is not in nav.json`);
  }
}

if (problems.length) {
  console.error("stale or unlisted:\n" + problems.join("\n"));
  process.exit(1);
}
