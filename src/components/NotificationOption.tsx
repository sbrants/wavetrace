import type { ReactNode } from "react";

type NotificationOptionProps = {
  id: string;
  label: string;
  description: string;
  checked?: boolean;
  onChange?: (checked: boolean) => void;
  disabled?: boolean;
  control?: ReactNode;
};

/** One notification setting: checkbox toggle or custom control + help text. */
export default function NotificationOption({
  id,
  label,
  description,
  checked,
  onChange,
  disabled,
  control,
}: NotificationOptionProps) {
  return (
    <div className="notification-option">
      <div className="notification-option-control">
        {control ?? (
          <input
            id={id}
            type="checkbox"
            checked={checked ?? false}
            disabled={disabled}
            onChange={(e) => onChange?.(e.target.checked)}
          />
        )}
      </div>
      <div className="notification-option-body">
        <label htmlFor={control ? undefined : id} className="notification-option-label">
          {label}
        </label>
        <p className="notification-option-desc">{description}</p>
      </div>
    </div>
  );
}
