import { ReactNode, useId } from "react";

/** A text input with a visible label and an optional error, wired for screen readers. */
export function TextField(props: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  error?: string | null;
  hint?: string;
  inputMode?: "decimal" | "numeric" | "text";
}) {
  const id = useId();
  const errId = `${id}-err`;
  const hintId = `${id}-hint`;
  const describedBy = [props.error ? errId : null, props.hint ? hintId : null].filter(Boolean).join(" ") || undefined;
  return (
    <div className="field">
      <label htmlFor={id}>{props.label}</label>
      <input
        id={id}
        value={props.value}
        inputMode={props.inputMode ?? "decimal"}
        aria-invalid={props.error ? true : undefined}
        aria-describedby={describedBy}
        onChange={(e) => props.onChange(e.target.value)}
      />
      {props.hint && (
        <span id={hintId} className="hint">
          {props.hint}
        </span>
      )}
      {props.error && (
        <span id={errId} className="field-error">
          {props.error}
        </span>
      )}
    </div>
  );
}

export function SelectField<T extends string | number>(props: {
  label: string;
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
}) {
  const id = useId();
  return (
    <div className="field">
      <label htmlFor={id}>{props.label}</label>
      <select
        id={id}
        value={String(props.value)}
        onChange={(e) => {
          const o = props.options.find((x) => String(x.value) === e.target.value);
          if (o) props.onChange(o.value);
        }}
      >
        {props.options.map((o) => (
          <option key={String(o.value)} value={String(o.value)}>
            {o.label}
          </option>
        ))}
      </select>
    </div>
  );
}

/** A labelled read-only value in a definition list. */
export function Stat({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="stat">
      <dt>{label}</dt>
      <dd>{children}</dd>
    </div>
  );
}

/** Wraps a table so it scrolls inside its box on narrow screens, never the page. */
export function TableScroll({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="table-scroll" role="region" aria-label={label} tabIndex={0}>
      {children}
    </div>
  );
}
