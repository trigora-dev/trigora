type TccValue = {
  t: string;
  v?: unknown;
};

/** Plain JavaScript value from the host's TCC value JSON. */
export function toPlain(encoded: string): unknown {
  return decode(JSON.parse(encoded) as TccValue);
}

/** TCC value JSON for a handler result. The host still speaks `encode_value`. */
export function fromPlain(value: unknown): string {
  return JSON.stringify(encode(value));
}

function decode(value: TccValue): unknown {
  switch (value.t) {
    case 'undefined':
      return undefined;
    case 'null':
      return null;
    case 'bool':
    case 'number':
    case 'string':
      return value.v;
    case 'object': {
      const fields = value.v as Record<string, TccValue>;
      const object: Record<string, unknown> = {};
      for (const key of Object.keys(fields)) {
        const field = fields[key];
        if (field) {
          object[key] = decode(field);
        }
      }
      return object;
    }
    case 'array':
      return (value.v as TccValue[]).map((item) => decode(item));
    default:
      throw new Error(`unsupported TCC value \`${value.t}\``);
  }
}

function encode(value: unknown): TccValue {
  if (value === undefined) {
    return { t: 'undefined' };
  }
  if (value === null) {
    return { t: 'null' };
  }
  if (typeof value === 'boolean') {
    return { t: 'bool', v: value };
  }
  if (typeof value === 'number') {
    return { t: 'number', v: value };
  }
  if (typeof value === 'string') {
    return { t: 'string', v: value };
  }
  if (Array.isArray(value)) {
    return { t: 'array', v: value.map((item) => encode(item)) };
  }
  if (typeof value === 'object') {
    const fields: Record<string, TccValue> = {};
    for (const [key, item] of Object.entries(value)) {
      fields[key] = encode(item);
    }
    return { t: 'object', v: fields };
  }
  throw new Error(`cannot encode ${typeof value} as a TCC value`);
}
