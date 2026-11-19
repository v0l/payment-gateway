import { expect, test } from 'bun:test';
import { formatCountdown, formatMoney, pickLanguage } from './format';

test('language priority is query, browser, project default, english', () => {
  expect(pickLanguage('fr', 'de', ['es-ES'])).toBe('fr');
  expect(pickLanguage(null, 'de', ['es-ES'])).toBe('es');
  expect(pickLanguage(null, 'de', ['ja-JP'])).toBe('de');
  expect(pickLanguage(null, null, ['ja-JP'])).toBe('en');
  expect(pickLanguage('xx', null, [])).toBe('en');
  expect(pickLanguage(null, null, ['ga-IE'])).toBe('ga');
});

test('money is formatted for the locale', () => {
  expect(formatMoney(1500, 'EUR', 'de')).toMatch(/15,00/);
  expect(formatMoney(1500, 'EUR', 'en-US')).toMatch(/^€15\.00$/);
  expect(formatMoney(1234500, 'EUR', 'fr')).toMatch(/12\s?345,00/);
  expect(formatMoney(5_000_000, 'BTC', 'en')).toBe('5,000 sats');
  expect(formatMoney(5_000_001, 'BTC', 'en')).toBe('5,001 sats');
});

test('countdown formatting', () => {
  expect(formatCountdown(59_000)).toBe('0:59');
  expect(formatCountdown(61_000)).toBe('1:01');
  expect(formatCountdown(3_661_000)).toBe('1:01:01');
  expect(formatCountdown(-5)).toBe('0:00');
});
