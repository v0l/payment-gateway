import type { ComponentChildren } from 'preact';
import { BrandLink, Btn, SkipLink } from '../shared/components';

export interface NavPage {
  slug: string;
  title: string;
}

export interface NavSection {
  id: string;
  title: string;
  banner?: string;
  pages: NavPage[];
}

export interface Assets {
  styles: string[];
  script?: string;
}

const TOP_TABS: [string, string, string][] = [
  ['/guide/', 'Guide', 'guide'],
  ['/operator/', 'Operators', 'operator'],
  ['/docs', 'API reference', 'docs'],
  ['/legal/', 'Legal', 'legal'],
];

const pageHref = (section: string, slug: string) => (slug === 'index' ? `/${section}/` : `/${section}/${slug}`);

export function TopBar(props: { tabs: [string, string, string][]; active?: string }) {
  return (
    <header class="top">
      <div class="top-in">
        <BrandLink href="/" />
        <nav class="tabs" aria-label="Primary">
          {props.tabs.map(([href, label, id]) => (
            <a href={href} aria-current={id === props.active ? 'page' : undefined} key={href}>{label}</a>
          ))}
        </nav>
        <Btn href="/console" primary>Sign in</Btn>
      </div>
    </header>
  );
}

export function SiteFooter() {
  return (
    <footer>
      <div class="foot-in">
        <span>Payment Gateway</span>
        <nav aria-label="Legal">
          <ul>
            <li><a href="/legal/terms">Terms</a></li>
            <li><a href="/legal/privacy">Privacy</a></li>
            <li><a href="/legal/acceptable-use">Acceptable use</a></li>
            <li><a href="/legal/custody">Custodial and non-custodial</a></li>
          </ul>
        </nav>
      </div>
    </footer>
  );
}

export function Document(props: { title: string; description?: string; assets: Assets; children: ComponentChildren; script?: boolean }) {
  return (
    <html lang="en">
      <head>
        <meta charSet="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        {props.description ? <meta name="description" content={props.description} /> : null}
        <title>{props.title}</title>
        <link rel="icon" href="/favicon.svg" type="image/svg+xml" />
        {props.assets.styles.map(s => (
          <link rel="stylesheet" href={s} key={s} />
        ))}
      </head>
      <body>
        {props.children}
        {props.assets.script ? <script type="module" src={props.assets.script} /> : null}
      </body>
    </html>
  );
}

export function DocsShell(props: {
  nav: NavSection[];
  section: string;
  slug: string;
  active: string;
  banner?: string;
  children: ComponentChildren;
}) {
  const here = props.nav.find(s => s.id === props.section);
  return (
    <>
      <SkipLink target="#content" />
      <TopBar tabs={TOP_TABS} active={props.active} />
      {here ? (
        <nav class="subtabs" aria-label={`${here.title} pages`}>
          <ul>
            {here.pages.map(p => (
              <li key={p.slug}>
                <a href={pageHref(here.id, p.slug)} aria-current={p.slug === props.slug ? 'page' : undefined}>{p.title}</a>
              </li>
            ))}
          </ul>
        </nav>
      ) : null}
      <div class="layout">
        <nav class="side" aria-label="Documentation">
          {props.nav.map(s => (
            <>
              <h2>{s.title}</h2>
              <ul>
                {s.pages.map(p => (
                  <li key={p.slug}>
                    <a href={pageHref(s.id, p.slug)} aria-current={s.id === props.section && p.slug === props.slug ? 'page' : undefined}>{p.title}</a>
                  </li>
                ))}
              </ul>
            </>
          ))}
        </nav>
        <main id="content">
          {props.banner ? <p class="draft" role="note">{props.banner}</p> : null}
          {props.children}
        </main>
      </div>
      <SiteFooter />
    </>
  );
}

export const LANDING_TABS: [string, string, string][] = [
  ['/guide/', 'Docs', 'guide'],
  ['/docs', 'API reference', 'docs'],
  ['/operator/', 'Operators', 'operator'],
  ['#pricing', 'Pricing', 'pricing'],
];
