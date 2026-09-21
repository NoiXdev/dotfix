import { useCombobox } from "downshift";
import { useMemo, useState } from "react";

export type SelectOption = { value: string; label: string };

/**
 * A dropdown with a search field, replacing the native `<select>` dotfix used
 * to render.
 *
 * Built on downshift's `useCombobox` rather than by hand: the keyboard and
 * ARIA behaviour a listbox owes a screen reader (roles, `aria-activedescendant`,
 * focus that stays on the input while the highlight moves) is a lot of detail
 * to get right and easy to get subtly wrong.
 *
 * Every dropdown gets the search field, including the two-option ones. That is
 * a deliberate house decision: one control that always behaves the same way
 * beats one that changes shape depending on how many options it happens to
 * have today — the set picker below grows as the repository does.
 */
export default function Select({
  options,
  value,
  onChange,
  label,
  ariaLabel,
  compact = false,
}: {
  options: SelectOption[];
  value: string;
  onChange: (value: string) => void;
  /** Rendered as the field's visible label. Omit when `ariaLabel` is used. */
  label?: string;
  /** Accessible name when there is no room for a visible label. */
  ariaLabel?: string;
  /** Inline sizing for use inside a list row. */
  compact?: boolean;
}) {
  const selected = options.find((o) => o.value === value) ?? null;
  const [query, setQuery] = useState("");

  const matches = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q
      ? options.filter((o) => o.label.toLowerCase().includes(q))
      : options;
  }, [options, query]);

  const {
    isOpen,
    highlightedIndex,
    getLabelProps,
    getToggleButtonProps,
    getInputProps,
    getMenuProps,
    getItemProps,
  } = useCombobox({
    items: matches,
    selectedItem: selected,
    itemToString: (item) => item?.label ?? "",
    initialInputValue: selected?.label ?? "",
    onInputValueChange: ({ inputValue }) => setQuery(inputValue ?? ""),
    onSelectedItemChange: ({ selectedItem }) => {
      if (selectedItem) onChange(selectedItem.value);
    },
    // The input doubles as the closed control and as the search box, so its
    // text has to mean two different things. Opening clears it, or the
    // current selection would sit there filtering the list down to itself —
    // you would open a dropdown and see one option. Closing puts the
    // selection back, so the field reads as what is chosen rather than as
    // whatever was half-typed before the menu went away.
    stateReducer: (state, { type, changes }) => {
      switch (type) {
        case useCombobox.stateChangeTypes.ToggleButtonClick:
        case useCombobox.stateChangeTypes.InputClick:
          return { ...changes, inputValue: "" };
        case useCombobox.stateChangeTypes.InputBlur:
        case useCombobox.stateChangeTypes.ItemClick:
        case useCombobox.stateChangeTypes.InputKeyDownEnter:
        case useCombobox.stateChangeTypes.InputKeyDownEscape:
          return {
            ...changes,
            inputValue:
              (changes.selectedItem ?? state.selectedItem)?.label ?? "",
          };
        default:
          return changes;
      }
    },
  });

  const field = compact
    ? "w-32 rounded-md border border-control bg-surface px-1 py-0.5 text-xs text-ink"
    : "w-full rounded-md border border-control bg-surface px-2 py-1 text-sm text-ink";

  return (
    <div className={compact ? "relative shrink-0" : "flex flex-col gap-1"}>
      {label ? (
        <label {...getLabelProps()} className="text-sm font-medium text-ink">
          {label}
        </label>
      ) : null}
      <div className={compact ? "" : "relative"}>
        <input
          {...getInputProps({
            "aria-label": label ? undefined : ariaLabel,
            readOnly: !isOpen,
          })}
          className={`${field} pr-5`}
        />
        <button
          type="button"
          {...getToggleButtonProps()}
          aria-hidden="true"
          className="absolute inset-y-0 right-1 flex items-center text-ink-muted"
        >
          <span aria-hidden="true" className="text-[0.6rem]">
            ▾
          </span>
        </button>
        <ul
          {...getMenuProps()}
          className={
            isOpen
              ? "absolute z-10 mt-1 max-h-48 w-full overflow-auto rounded-md border border-control bg-surface py-1 shadow-sm"
              : "hidden"
          }
        >
          {isOpen && matches.length === 0 ? (
            <li className="px-2 py-1 text-xs text-ink-muted">No match</li>
          ) : null}
          {isOpen &&
            matches.map((item, index) => (
              <li
                key={item.value}
                {...getItemProps({ item, index })}
                className={`cursor-default px-2 py-1 ${
                  compact ? "text-xs" : "text-sm"
                } ${
                  highlightedIndex === index ? "bg-canvas text-ink" : "text-ink"
                }`}
              >
                {item.label}
              </li>
            ))}
        </ul>
      </div>
    </div>
  );
}
