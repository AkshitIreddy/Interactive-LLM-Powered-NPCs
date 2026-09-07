import { useEffect, useId, useState, type ReactNode } from "react";

/** Ordinary navigation buttons retain mounted form state across sections. */
export function WorkspaceSections({
  label,
  sections,
  selectedId,
  onSelectionChange,
}: {
  label: string;
  sections: Array<{
    id: string;
    label: string;
    description?: string;
    content: ReactNode;
  }>;
  selectedId?: string;
  onSelectionChange?: (id: string) => void;
}) {
  const [localSelected, setSelected] = useState(sections[0].id);
  const selected = selectedId ?? localSelected;
  const id = useId();
  useEffect(() => {
    if (document.activeElement?.closest("[hidden]"))
      document.getElementById(`${id}-${selected}`)?.focus();
  }, [id, selected]);
  return (
    <div className="workspace-sections">
      <nav className="workspace-section-nav" aria-label={label}>
        {sections.map((section) => (
          <button
            key={section.id}
            aria-current={selected === section.id ? "page" : undefined}
            aria-controls={`${id}-${section.id}`}
            onClick={() => {
              setSelected(section.id);
              onSelectionChange?.(section.id);
            }}
          >
            <b>{section.label}</b>
            {section.description && <small>{section.description}</small>}
          </button>
        ))}
      </nav>
      {sections.map((section) => (
        <section
          key={section.id}
          id={`${id}-${section.id}`}
          hidden={selected !== section.id}
          tabIndex={-1}
          className="workspace-section-content"
          aria-label={section.label}
        >
          {section.content}
        </section>
      ))}
    </div>
  );
}
