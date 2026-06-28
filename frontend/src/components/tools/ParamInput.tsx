import type { ParamInfo } from './toolParamUtils';

interface Props {
  param: ParamInfo;
  value: string;
  onChange: (val: string) => void;
  contextReq: boolean;
}

export function ParamInput({ param, value, onChange, contextReq }: Props) {
  const showRequired = contextReq || param.required;

  if (param.enum) {
    return (
      <div className="tool-param-field">
        <div className="tool-param-label">
          <span className="tool-param-name">{param.name}</span>
          {showRequired && <span className="tool-param-required">*</span>}
        </div>
        {param.description && <div className="tool-param-hint">{param.description}</div>}
        <select className="tool-param-select" value={value} onChange={(e) => onChange(e.target.value)}>
          <option value="">{showRequired ? '请选择...' : '默认'}</option>
          {param.enum!.map((v) => (
            <option key={v} value={v}>{v}</option>
          ))}
        </select>
      </div>
    );
  }

  if (param.type === 'boolean') {
    const checked = value === 'true';
    return (
      <div className="tool-param-field tool-param-field-bool">
        <div className="tool-param-field-row">
          <div className="tool-param-label">
            <span className="tool-param-name">{param.name}</span>
            {showRequired && <span className="tool-param-required">*</span>}
          </div>
          <button type="button" className={`tool-param-toggle${checked ? ' on' : ''}`}
            onClick={() => onChange(checked ? 'false' : 'true')} role="switch" aria-checked={checked}>
            <span className="tool-param-toggle-track">
              <span className="tool-param-toggle-knob">
                <span className="toggle-icon toggle-icon-off">
                  <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round">
                    <line x1="5" x2="19" y1="12" y2="12" />
                  </svg>
                </span>
                <span className="toggle-icon toggle-icon-on">
                  <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
                    <polyline points="20 6 9 17 4 12" />
                  </svg>
                </span>
              </span>
            </span>
          </button>
        </div>
        {param.description && <div className="tool-param-hint">{param.description}</div>}
      </div>
    );
  }

  if (param.type === 'integer') {
    return (
      <div className="tool-param-field">
        <div className="tool-param-label">
          <span className="tool-param-name">{param.name}</span>
          {showRequired && <span className="tool-param-required">*</span>}
        </div>
        {param.description && <div className="tool-param-hint">{param.description}</div>}
        <input type="number" className="tool-param-input" placeholder={param.description || param.name}
          value={value} onChange={(e) => onChange(e.target.value)} />
      </div>
    );
  }

  return (
    <div className="tool-param-field">
      <div className="tool-param-label">
        <span className="tool-param-name">{param.name}</span>
        {showRequired && <span className="tool-param-required">*</span>}
      </div>
      {param.description && <div className="tool-param-hint">{param.description}</div>}
      <input type="text" className="tool-param-input" placeholder={param.description || param.name}
        value={value} onChange={(e) => onChange(e.target.value)} />
    </div>
  );
}
