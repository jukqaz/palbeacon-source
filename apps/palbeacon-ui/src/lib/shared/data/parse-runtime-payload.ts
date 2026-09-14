import { safeParse, type GenericSchema } from 'valibot';

export const parseRuntimePayload = <T>(
  schema: GenericSchema,
  payload: unknown,
  label: string,
): T => {
  const result = safeParse(schema, payload, { abortEarly: true });
  if (!result.success) throw new Error(`${label} 형식이 올바르지 않습니다.`);
  return result.output as T;
};
