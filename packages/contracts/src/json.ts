export type JsonObject = {
  [key: string]: JsonValue | undefined;
};

export type JsonValue = string | number | boolean | null | JsonObject | JsonValue[];
