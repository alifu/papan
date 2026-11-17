// Chai-style `expect` (a subset). A function-body fragment: `sandbox.rs` wraps this file and
// `prelude.js` in one function, so nothing here becomes a global.

class AssertionError extends Error {
  constructor(message) {
    super(message);
    this.name = 'AssertionError';
  }
}

const SHOW_LIMIT = 200;

const show = (value) => {
  let text;
  try {
    text = typeof value === 'function' ? '[Function]' : JSON.stringify(value);
  } catch (_) {
    text = undefined;
  }
  if (typeof text !== 'string') text = String(value);
  return text.length > SHOW_LIMIT ? text.slice(0, SHOW_LIMIT) + '…' : text;
};

const typeOf = (value) => {
  if (value === null) return 'null';
  if (Array.isArray(value)) return 'array';
  if (typeof value !== 'object') return typeof value;
  return Object.prototype.toString.call(value).slice(8, -1).toLowerCase();
};

const same = (a, b) => a === b || (a !== a && b !== b);

const deepEqual = (a, b) => {
  if (same(a, b)) return true;
  const type = typeOf(a);
  if (type !== typeOf(b)) return false;
  if (type === 'date') return a.getTime() === b.getTime();
  if (type === 'regexp') return String(a) === String(b);
  if (type === 'array') {
    return a.length === b.length && a.every((item, i) => deepEqual(item, b[i]));
  }
  if (type !== 'object') return false;
  const keys = Object.keys(a);
  return (
    keys.length === Object.keys(b).length &&
    keys.every((key) => Object.prototype.hasOwnProperty.call(b, key) && deepEqual(a[key], b[key]))
  );
};

const sizeOf = (value) => {
  if (value == null) return undefined;
  if (typeof value === 'string' || Array.isArray(value)) return value.length;
  if (typeOf(value) === 'map' || typeOf(value) === 'set') return value.size;
  if (typeof value === 'object') return Object.keys(value).length;
  return undefined;
};

class Assertion {
  constructor(value, negated, deep) {
    this._value = value;
    this._negated = negated === true;
    this._deep = deep === true;
  }

  // `phrase` completes "expected <value> [not] to <phrase>".
  _assert(passed, phrase) {
    if (this._negated ? passed : !passed) {
      const not = this._negated ? 'not ' : '';
      throw new AssertionError('expected ' + show(this._value) + ' ' + not + 'to ' + phrase);
    }
    return this;
  }

  _equals(a, b) {
    return this._deep ? deepEqual(a, b) : same(a, b);
  }

  get not() {
    return new Assertion(this._value, !this._negated, this._deep);
  }

  get deep() {
    return new Assertion(this._value, this._negated, true);
  }

  equal(expected) {
    const phrase = (this._deep ? 'deeply equal ' : 'equal ') + show(expected);
    return this._assert(this._equals(this._value, expected), phrase);
  }

  eql(expected) {
    return this._assert(deepEqual(this._value, expected), 'deeply equal ' + show(expected));
  }

  a(type) {
    return this._assert(typeOf(this._value) === String(type).toLowerCase(), 'be a ' + type);
  }

  include(expected) {
    const value = this._value;
    let found = false;
    if (typeof value === 'string') {
      found = value.includes(String(expected));
    } else if (Array.isArray(value)) {
      found = value.some((item) => this._equals(item, expected));
    } else if (value !== null && typeof value === 'object' && expected !== null && typeof expected === 'object') {
      found = Object.keys(expected).every((key) => key in value && this._equals(value[key], expected[key]));
    }
    return this._assert(found, 'include ' + show(expected));
  }

  property(name, expected) {
    const value = this._value;
    const has = value != null && name in Object(value);
    if (arguments.length > 1) {
      this._assert(has && this._equals(value[name], expected), 'have property ' + show(name) + ' of ' + show(expected));
    } else {
      this._assert(has, 'have property ' + show(name));
    }
    return this._negated ? this : new Assertion(value[name], false, this._deep);
  }

  lengthOf(length) {
    return this._assert(sizeOf(this._value) === length, 'have a length of ' + show(length));
  }

  above(limit) {
    return this._assert(this._value > limit, 'be above ' + show(limit));
  }

  below(limit) {
    return this._assert(this._value < limit, 'be below ' + show(limit));
  }

  least(limit) {
    return this._assert(this._value >= limit, 'be at least ' + show(limit));
  }

  most(limit) {
    return this._assert(this._value <= limit, 'be at most ' + show(limit));
  }

  within(low, high) {
    return this._assert(this._value >= low && this._value <= high, 'be within ' + show(low) + '..' + show(high));
  }

  match(pattern) {
    return this._assert(pattern instanceof RegExp && pattern.test(this._value), 'match ' + String(pattern));
  }

  oneOf(list) {
    const found = Array.isArray(list) && list.some((item) => this._equals(item, this._value));
    return this._assert(found, 'be one of ' + show(list));
  }

  keys(...names) {
    const expected = names.length === 1 && Array.isArray(names[0]) ? names[0] : names;
    const actual = this._value != null && typeof this._value === 'object' ? Object.keys(this._value) : [];
    const matches = actual.length === expected.length && expected.every((key) => actual.includes(key));
    return this._assert(matches, 'have keys ' + show(expected));
  }

  throw(matcher) {
    let thrown = false;
    let error;
    try {
      this._value();
    } catch (caught) {
      thrown = true;
      error = caught;
    }
    const message = error != null && typeof error === 'object' ? String(error.message) : String(error);
    let matches = thrown;
    if (thrown && typeof matcher === 'string') matches = message.includes(matcher);
    else if (thrown && matcher instanceof RegExp) matches = matcher.test(message);
    else if (thrown && typeof matcher === 'function') matches = error instanceof matcher;
    return this._assert(matches, 'throw' + (matcher === undefined ? '' : ' ' + String(matcher)));
  }
}

const define = (names, descriptor) => {
  for (const name of names) Object.defineProperty(Assertion.prototype, name, descriptor);
};

// Words that only make a chain read well.
define(
  ['to', 'be', 'been', 'is', 'that', 'which', 'and', 'has', 'have', 'with', 'at', 'of', 'same', 'does', 'still', 'also'],
  { get() { return this; } },
);

const CHECKS = {
  ok: [(v) => Boolean(v), 'be truthy'],
  true: [(v) => v === true, 'be true'],
  false: [(v) => v === false, 'be false'],
  null: [(v) => v === null, 'be null'],
  undefined: [(v) => v === undefined, 'be undefined'],
  NaN: [(v) => v !== v, 'be NaN'],
  exist: [(v) => v !== null && v !== undefined, 'exist'],
  empty: [(v) => sizeOf(v) === 0, 'be empty'],
};
for (const name of Object.keys(CHECKS)) {
  const [check, phrase] = CHECKS[name];
  define([name], { get() { return this._assert(check(this._value), phrase); } });
}

const ALIASES = {
  equal: ['equals', 'eq'],
  eql: ['eqls'],
  a: ['an'],
  include: ['includes', 'contain', 'contains'],
  above: ['gt', 'greaterThan'],
  below: ['lt', 'lessThan'],
  least: ['gte'],
  most: ['lte'],
  match: ['matches'],
  throw: ['throws'],
};
for (const name of Object.keys(ALIASES)) {
  define(ALIASES[name], { value: Assertion.prototype[name], writable: true, configurable: true });
}

const expect = (value) => new Assertion(value, false, false);
