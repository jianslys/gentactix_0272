import React from 'react';

export const UiIdContext = React.createContext<boolean>(true);

export interface UiTagProps {
  id: string;
  kind?: 'win' | 'ctl' | 'opt';
  source?: string;
}

/** 窗口 / 控件 / 下拉选项可见 ID 角标：大王在界面上看到 ID，就能精准指挥改动对应源码。 */
export const UiTag: React.FC<UiTagProps> = ({ id, kind = 'ctl', source }) => {
  const show = React.useContext(UiIdContext);
  if (!show) return null;
  const cls =
    kind === 'win' ? 'ui-id-tag ui-id-win' : kind === 'opt' ? 'ui-id-tag ui-id-opt' : 'ui-id-tag ui-id-ctl';
  return (
    <span
      className={cls}
      data-ui-tag={id}
      title={source ? `UI ID: ${id} · 源码: ${source}` : `UI ID: ${id}`}
    >
      {id}
    </span>
  );
};
