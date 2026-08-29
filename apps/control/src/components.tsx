import type { ReactNode } from "react";
import { Button, Switch, Tooltip, TooltipTrigger } from "react-aria-components";
import { Icon, type IconName } from "./icons";

export function ActionButton({
  children,
  icon,
  variant = "primary",
  className = "",
  ...props
}: Omit<React.ComponentProps<typeof Button>, "children"> & {
  children?: ReactNode;
  icon?: IconName;
  variant?: "primary" | "quiet" | "danger" | "outline";
}) {
  return (
    <Button
      {...props}
      className={`action-button action-button--${variant} ${className}`}
    >
      {icon && <Icon name={icon} size={17} />}
      {children}
    </Button>
  );
}

export function IconButton({
  label,
  icon,
  ...props
}: Omit<React.ComponentProps<typeof Button>, "children"> & {
  label: string;
  icon: IconName;
}) {
  return (
    <TooltipTrigger delay={500}>
      <Button {...props} className="icon-button" aria-label={label}>
        <Icon name={icon} size={18} />
      </Button>
      <Tooltip placement="bottom" className="tooltip">
        {label}
      </Tooltip>
    </TooltipTrigger>
  );
}

export function Toggle({
  label,
  description,
  isSelected,
  onChange,
  privacy,
  disabled = false,
}: {
  label: string;
  description: string;
  isSelected: boolean;
  onChange: (selected: boolean) => void;
  privacy?: string;
  disabled?: boolean;
}) {
  return (
    <Switch
      isSelected={isSelected}
      onChange={onChange}
      isDisabled={disabled}
      className="setting-toggle"
    >
      <span className="setting-toggle__copy">
        <span className="setting-toggle__label">{label}</span>
        <span className="setting-toggle__description">{description}</span>
        {privacy && (
          <span className="setting-toggle__privacy">
            <Icon name="shield" size={13} />
            {privacy}
          </span>
        )}
      </span>
      <span className="switch-track" aria-hidden="true">
        <span className="switch-thumb" />
      </span>
    </Switch>
  );
}

export function Tip({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <TooltipTrigger delay={350}>
      <span className="tip-trigger" tabIndex={0} aria-label={label}>
        {children}
      </span>
      <Tooltip placement="top" className="tooltip tooltip--wide">
        {label}
      </Tooltip>
    </TooltipTrigger>
  );
}

export function StatusPill({
  tone = "neutral",
  children,
  dot = true,
}: {
  tone?: "neutral" | "ok" | "warn" | "danger" | "teal" | "purple";
  children: ReactNode;
  dot?: boolean;
}) {
  return (
    <span className={`status-pill status-pill--${tone}`}>
      {dot && <span className="status-pill__dot" />}
      {children}
    </span>
  );
}

export function Metric({
  label,
  value,
  detail,
  tone = "default",
}: {
  label: string;
  value: string;
  detail?: string;
  tone?: "default" | "good" | "warn";
}) {
  return (
    <div className={`metric metric--${tone}`}>
      <span className="metric__label">{label}</span>
      <strong>{value}</strong>
      {detail && <small>{detail}</small>}
    </div>
  );
}

export function SectionTitle({
  eyebrow,
  title,
  description,
  action,
}: {
  eyebrow?: string;
  title: string;
  description?: string;
  action?: ReactNode;
}) {
  return (
    <div className="section-heading">
      <div>
        {eyebrow && <div className="eyebrow">{eyebrow}</div>}
        <h2>{title}</h2>
        {description && <p>{description}</p>}
      </div>
      {action && <div className="section-heading__action">{action}</div>}
    </div>
  );
}

export function MiniBar({
  value,
  tone = "teal",
  label,
}: {
  value: number;
  tone?: "teal" | "amber" | "purple";
  label?: string;
}) {
  return (
    <div className="mini-bar" aria-label={label} role="img">
      <span
        className={`mini-bar__fill mini-bar__fill--${tone}`}
        style={{ width: `${Math.max(2, Math.min(100, value))}%` }}
      />
    </div>
  );
}

export function EmptyState({
  icon,
  title,
  description,
  action,
}: {
  icon: IconName;
  title: string;
  description: string;
  action?: ReactNode;
}) {
  return (
    <div className="empty-state">
      <div className="empty-state__icon">
        <Icon name={icon} size={26} />
      </div>
      <h2>{title}</h2>
      <p>{description}</p>
      {action}
    </div>
  );
}

export function Disclosure({
  tone = "privacy",
  title,
  children,
}: {
  tone?: "privacy" | "warning" | "info";
  title: string;
  children: ReactNode;
}) {
  return (
    <div className={`disclosure disclosure--${tone}`}>
      <Icon
        name={
          tone === "warning"
            ? "warning"
            : tone === "privacy"
              ? "shield"
              : "spark"
        }
        size={18}
      />
      <div>
        <strong>{title}</strong>
        <p>{children}</p>
      </div>
    </div>
  );
}

export function Skeleton({
  lines = 3,
  compact = false,
}: {
  lines?: number;
  compact?: boolean;
}) {
  return (
    <div
      className={`skeleton ${compact ? "skeleton--compact" : ""}`}
      aria-label="Loading"
      role="status"
    >
      <span className="skeleton__title" />
      {Array.from({ length: lines }, (_, i) => (
        <span
          key={i}
          className="skeleton__line"
          style={{ width: `${82 - (i % 3) * 13}%` }}
        />
      ))}
    </div>
  );
}

export function KeyboardKey({ children }: { children: ReactNode }) {
  return <kbd>{children}</kbd>;
}
