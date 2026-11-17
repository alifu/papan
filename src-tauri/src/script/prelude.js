// The `pm` API, `console`, `atob` and `btoa`, built on the single native function `host`.
// A function-body fragment (see expect.js). Only strings cross to the host, and the host
// checks every one of them: nothing in this file is a security boundary.

const call = (op, ...args) => host(op, ...args);

const text = (value) => {
  if (typeof value === 'string') return value;
  let out;
  if (value !== null && typeof value === 'object') {
    try {
      out = JSON.stringify(value);
    } catch (_) {
      out = undefined;
    }
  }
  return typeof out === 'string' ? out : String(value);
};

const key = (name) => {
  if (typeof name !== 'string') throw new TypeError('A variable name must be a string.');
  return name;
};

const orUndefined = (value) => (value == null ? undefined : value);

const scope = (readId, writeId) =>
  Object.freeze({
    get: (name) => orUndefined(call('var.get', readId, key(name))),
    has: (name) => call('var.has', readId, key(name)) === '1',
    set: (name, value) => void call('var.set', writeId, key(name), text(value)),
    unset: (name) => void call('var.unset', writeId, key(name)),
    toObject: () => JSON.parse(call('var.all', readId)),
  });

// Header lists are arrays of [name, value]; names compare without case.
const lower = (name) => String(name).toLowerCase();
const headerReader = (load) => ({
  get: (name) => {
    const found = load().find((pair) => lower(pair[0]) === lower(name));
    return found ? found[1] : undefined;
  },
  has: (name) => load().some((pair) => lower(pair[0]) === lower(name)),
  toObject: () => Object.fromEntries(load()),
  each: (fn) => load().forEach((pair) => fn({ key: pair[0], value: pair[1] })),
});

const headerPair = (first, second) =>
  first !== null && typeof first === 'object' ? [text(first.key), text(first.value)] : [text(first), text(second)];

const loadRequest = () => JSON.parse(call('req.get'));
const updateRequest = (change) => void call('req.set', JSON.stringify(change(loadRequest())));
const withHeaders = (change) => updateRequest((request) => ({ ...request, headers: change(request.headers) }));
const setBody = (value) => {
  if (value == null) call('req.setBody');
  else call('req.setBody', text(typeof value === 'object' && 'raw' in value ? value.raw : value));
};

const requestBody = Object.freeze({
  mode: 'raw',
  get raw() {
    return orUndefined(call('req.body'));
  },
  set raw(value) {
    setBody(value);
  },
  update: setBody,
  toString: () => call('req.body') ?? '',
});

const request = Object.freeze({
  get url() {
    return loadRequest().url;
  },
  set url(value) {
    updateRequest((current) => ({ ...current, url: text(value) }));
  },
  get method() {
    return loadRequest().method;
  },
  set method(value) {
    updateRequest((current) => ({ ...current, method: text(value) }));
  },
  get body() {
    return requestBody;
  },
  set body(value) {
    setBody(value);
  },
  headers: Object.freeze({
    ...headerReader(() => loadRequest().headers),
    add: (first, second) => withHeaders((headers) => [...headers, headerPair(first, second)]),
    upsert: (first, second) => {
      const pair = headerPair(first, second);
      withHeaders((headers) => [...headers.filter((item) => lower(item[0]) !== lower(pair[0])), pair]);
    },
    remove: (name) => withHeaders((headers) => headers.filter((item) => lower(item[0]) !== lower(name))),
  }),
});

const responseAssertions = (meta, body, negated) => {
  const assert = (passed, phrase) => {
    if (negated ? passed : !passed) {
      throw new AssertionError('expected response ' + (negated ? 'not ' : '') + 'to ' + phrase);
    }
  };
  const headers = headerReader(() => meta.headers);
  const be = {};
  const ranges = { ok: [200, 299], success: [200, 299], clientError: [400, 499], serverError: [500, 599], error: [400, 599] };
  for (const name of Object.keys(ranges)) {
    const [low, high] = ranges[name];
    Object.defineProperty(be, name, {
      get: () => assert(meta.code >= low && meta.code <= high, 'be ' + name + ' (status ' + meta.code + ')'),
    });
  }
  return {
    be,
    have: {
      status: (expected) =>
        assert(
          typeof expected === 'number' ? meta.code === expected : meta.status === expected,
          'have status ' + expected + ' (got ' + meta.code + ' ' + meta.status + ')',
        ),
      header: (name, value) =>
        assert(
          headers.has(name) && (value === undefined || headers.get(name) === value),
          'have header ' + name,
        ),
      body: (expected) => assert(body() === expected, 'have the given body'),
    },
    get not() {
      return responseAssertions(meta, body, !negated);
    },
  };
};

const makeResponse = (meta, readBody) => {
  let cached;
  const body = () => (cached === undefined ? (cached = readBody()) : cached);
  return Object.freeze({
    code: meta.code,
    status: meta.status,
    responseTime: meta.responseTime,
    headers: Object.freeze(headerReader(() => meta.headers)),
    text: body,
    json: () => JSON.parse(body()),
    to: responseAssertions(meta, body, false),
  });
};

const describeError = (error) =>
  error instanceof Error ? (error.name === 'AssertionError' ? error.message : error.name + ': ' + error.message) : text(error);

const sendHeaders = (spec) => {
  const given = spec.header ?? spec.headers ?? [];
  if (Array.isArray(given)) return given.map((item) => (Array.isArray(item) ? headerPair(item[0], item[1]) : headerPair(item)));
  if (typeof given === 'object') return Object.keys(given).map((name) => [name, text(given[name])]);
  throw new TypeError('pm.sendRequest: headers must be an array or an object.');
};

const sendBody = (body) => {
  if (body == null) return null;
  if (typeof body === 'string') return body;
  if (typeof body === 'object' && (body.mode === undefined || body.mode === 'raw') && 'raw' in body) return text(body.raw);
  throw new TypeError('pm.sendRequest: only raw bodies are supported.');
};

const sendRequest = (input, callback) => {
  const spec = typeof input === 'string' ? { url: input } : input;
  if (spec === null || typeof spec !== 'object') throw new TypeError('pm.sendRequest: expected a URL or an options object.');
  const wire = { method: text(spec.method ?? 'GET'), url: text(spec.url), headers: sendHeaders(spec), body: sendBody(spec.body) };
  const result = JSON.parse(call('send', JSON.stringify(wire)));
  const error = result.ok ? null : new Error(result.error);
  if (typeof callback !== 'function') {
    if (error) throw error;
    return;
  }
  if (error) callback(error, undefined);
  else callback(null, makeResponse(result.response, () => result.response.body));
};

const test = (name, fn) => {
  const label = text(name);
  if (typeof fn !== 'function') throw new TypeError('pm.test: the second argument must be a function.');
  try {
    const result = fn();
    if (result !== null && typeof result === 'object' && typeof result.then === 'function') {
      throw new Error('asynchronous tests are not supported');
    }
  } catch (error) {
    call('test', label, '0', describeError(error));
    return;
  }
  call('test', label, '1');
};

const responseMeta = call('res.get');
const pm = Object.freeze({
  environment: scope('environment', 'environment'),
  collectionVariables: scope('collection', 'collection'),
  globals: scope('globals', 'globals'),
  variables: scope('any', 'runtime'),
  request,
  response: responseMeta == null ? undefined : makeResponse(JSON.parse(responseMeta), () => call('res.body')),
  test,
  expect,
  sendRequest,
});

const formatLog = (value) => (value instanceof Error ? value.name + ': ' + value.message : text(value));
const logger = (level) => (...values) => void call('console', level, values.map(formatLog).join(' '));

Object.defineProperty(globalThis, 'pm', { value: pm, writable: false, configurable: false, enumerable: true });
globalThis.console = Object.freeze({
  log: logger('log'),
  debug: logger('log'),
  info: logger('info'),
  warn: logger('warn'),
  error: logger('error'),
});
globalThis.btoa = (value) => call('btoa', String(value));
globalThis.atob = (value) => call('atob', String(value));
