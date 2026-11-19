import type { ComponentChildren, JSX } from 'preact';

export function Mark() {
  return <span class="mark" />;
}

export function SkipLink(props: { target: string }) {
  return <a class="skip" href={props.target}>Skip to content</a>;
}

export function Btn(props: { href: string; primary?: boolean; big?: boolean; children: ComponentChildren }) {
  const cls = ['btn', props.primary ? 'primary' : '', props.big ? 'big' : ''].filter(Boolean).join(' ');
  return <a class={cls} href={props.href}>{props.children}</a>;
}

export function BrandLink(props: { href: string; children?: ComponentChildren }): JSX.Element {
  return (
    <a class="brand" href={props.href}>
      <Mark />
      <span class="name">{props.children ?? 'Payment Gateway'}</span>
    </a>
  );
}
