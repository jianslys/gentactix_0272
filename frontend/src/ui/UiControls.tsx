import React from 'react';
import { UiIdContext, UiTag } from './UiTag';

export interface UiButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  uiId: string;
  badgeKind?: 'ctl' | 'opt';
  source?: string;
}

/** 带强制可见 UI ID 的傻瓜按钮封装：自动挂 data-ui-id 与 <UiTag>。 */
export const UiButton: React.FC<UiButtonProps> = ({
  uiId,
  badgeKind = 'ctl',
  source,
  children,
  type = 'button',
  ...rest
}) => (
  <button type={type} data-ui-id={uiId} {...rest}>
    <UiTag id={uiId} kind={badgeKind} source={source} />
    {children}
  </button>
);

export interface UiSelectOption {
  value: string;
  label: string;
  optId: string;
}

export interface UiSelectProps {
  uiId: string;
  value: string;
  onChangeValue: (val: string) => void;
  options: UiSelectOption[];
  badgeKind?: 'ctl' | 'opt';
  source?: string;
  className?: string;
  selectClassName?: string;
  testId?: string;
  ariaLabel?: string;
  disabled?: boolean;
}

/** 带全路径选项 ID 的下拉框封装：展开选项自动带 [W?.??.opt_*] 前缀（受 T12 开关控制）。 */
export const UiSelect: React.FC<UiSelectProps> = ({
  uiId,
  value,
  onChangeValue,
  options,
  badgeKind = 'ctl',
  source,
  className,
  selectClassName,
  testId,
  ariaLabel,
  disabled,
}) => {
  const showIds = React.useContext(UiIdContext);
  return (
    <span className={className ? `ui-select-wrap ${className}` : 'ui-select-wrap'} data-ui-id={uiId}>
      <UiTag id={uiId} kind={badgeKind} source={source} />
      <select
        className={selectClassName}
        data-testid={testId}
        aria-label={ariaLabel}
        value={value}
        disabled={disabled}
        onChange={(e) => onChangeValue(e.target.value)}
      >
        {options.map((opt) => (
          <option key={opt.value} value={opt.value} data-ui-id={opt.optId}>
            {showIds ? `[${opt.optId}] ${opt.label}` : opt.label}
          </option>
        ))}
      </select>
    </span>
  );
};

export interface UiToggleProps {
  uiId: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  optOnId?: string;
  optOffId?: string;
  testId?: string;
  source?: string;
}

/** 带可见 UI ID 的勾选开关封装。 */
export const UiToggle: React.FC<UiToggleProps> = ({
  uiId,
  checked,
  onChange,
  label,
  optOnId,
  optOffId,
  testId,
  source,
}) => {
  const activeOptId = checked ? optOnId : optOffId;
  return (
    <label className="chk-label" data-ui-id={uiId}>
      <UiTag id={uiId} kind="ctl" source={source} />
      <input
        type="checkbox"
        data-testid={testId}
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
      />
      <span>{label}</span>
      {activeOptId ? <UiTag id={activeOptId} kind="opt" /> : null}
    </label>
  );
};
