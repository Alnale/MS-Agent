import type { ToolDefinition, ToolParamSchema } from '../../api/types';

export interface ParamInfo {
  name: string;
  description: string;
  required: boolean;
  type: string;
  enum?: string[];
  default?: unknown;
}

type ParamDepMap = Record<string, Record<string, string[]>>;

export const TOOL_PARAM_DEPS: Record<string, ParamDepMap> = {
  docflow: {
    action: {
      convert: ['input_path', 'conversion_type', 'output_path'],
      start: [],
      status: ['job_id'],
    },
    conversion_type: {
      doc_to_pdf: ['image_dpi', 'lossless', 'page_size', 'orientation', 'embed_fonts'],
      pdf_to_docx: [],
      to_markdown: [],
    },
  },
};

const TOOL_CONTEXT_REQUIRED: Record<string, Record<string, string[]>> = {
  docflow: {
    convert: ['input_path', 'conversion_type'],
    status: ['job_id'],
  },
};

export function getAllParams(item: ToolDefinition): ParamInfo[] {
  if (!item.parameters?.schema) return [];
  const props = item.parameters.schema.properties as Record<string, ToolParamSchema> | undefined;
  if (!props) return [];
  const toolRequired = new Set(item.parameters.required ?? []);
  const allNames = Object.keys(props);
  allNames.sort((a, b) => {
    const aR = toolRequired.has(a) ? 0 : 1;
    const bR = toolRequired.has(b) ? 0 : 1;
    return aR !== bR ? aR - bR : a.localeCompare(b);
  });
  return allNames.map((name) => ({
    name,
    description: props[name]?.description ?? '',
    required: toolRequired.has(name),
    type: props[name]?.type ?? 'string',
    enum: props[name]?.enum,
    default: props[name]?.default,
  }));
}

export function getVisibleParams(
  allParams: ParamInfo[],
  toolName: string,
  values: Record<string, string>,
): ParamInfo[] {
  const deps = TOOL_PARAM_DEPS[toolName];
  if (!deps) return allParams;

  const childOf: Record<string, string> = {};
  for (const [parent, rules] of Object.entries(deps)) {
    for (const children of Object.values(rules)) {
      for (const child of children) {
        childOf[child] = parent;
      }
    }
  }

  return allParams.filter((p) => {
    const parent = childOf[p.name];
    if (!parent) return true;
    const parentValue = values[parent];
    if (!parentValue) return false;
    const rule = deps[parent]?.[parentValue];
    return rule?.includes(p.name) ?? false;
  });
}

export function isContextRequired(
  toolName: string,
  paramName: string,
  values: Record<string, string>,
): boolean {
  const ctxDeps = TOOL_CONTEXT_REQUIRED[toolName];
  if (!ctxDeps) return false;
  for (const [, requiredParams] of Object.entries(ctxDeps)) {
    if (requiredParams.includes(paramName)) {
      const deps = TOOL_PARAM_DEPS[toolName];
      if (deps) {
        for (const [parentParam, rules] of Object.entries(deps)) {
          for (const [parentValue, children] of Object.entries(rules)) {
            if (children.length > 0 && values[parentParam] === parentValue) {
              return true;
            }
          }
        }
      }
    }
  }
  return false;
}

export function abbreviateToolName(name: string): string {
  if (name.length <= 8) return name;
  const parts = name.split(/[_\-]/);
  if (parts.length > 1) return parts.slice(0, 2).join('');
  return name.slice(0, 6) + '…';
}
