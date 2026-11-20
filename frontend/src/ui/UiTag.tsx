// GenTactix V2 frontend · 可视化 UI 窗口/控件编号标牌（便于精准点名改进指定控件）

import React, { useContext } from 'react';

export const UiIdContext = React.createContext<boolean>(true);

export interface UiTagProps {
  id: string;
  kind?: 'win' | 'ctl';
}

export const UiTag: React.FC<UiTagProps> = ({ id, kind = 'ctl' }) => {
  const visible = useContext(UiIdContext);
  if (!visible) return null;
  return (
    <span
      data-ui-tag={id}
      className={kind === 'win' ? 'ui-id-tag ui-id-win' : 'ui-id-tag ui-id-ctl'}
    >
      {id}
    </span>
  );
};
