import { expect, test } from 'bun:test';
import { renderMarkdown } from './markdown';

test('headings get ids and inline code is escaped', () => {
  const html = renderMarkdown('# Title\n\nUse `<b>` and **bold**.');
  expect(html).toContain('<h1 id="title">Title</h1>');
  expect(html).toContain('<code>&lt;b&gt;</code>');
  expect(html).toContain('<strong>bold</strong>');
});

test('tables, lists and code blocks render', () => {
  const html = renderMarkdown('| A | B |\n| --- | --- |\n| 1 | 2 |\n\n- one\n- two\n\n```js\nlet a = 1 < 2;\n```');
  expect(html).toContain('<th scope="col">A</th>');
  expect(html).toContain('<td>2</td>');
  expect(html).toContain('<ul><li>one</li><li>two</li></ul>');
  expect(html).toContain('<code class="language-js">let a = 1 &lt; 2;</code>');
});

test('raw html in markdown is escaped', () => {
  expect(renderMarkdown('<script>alert(1)</script>')).not.toContain('<script>');
});
