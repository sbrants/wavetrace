import { changelogBody } from "../changelog";

function renderInline(text: string): React.ReactNode[] {
  const parts = text.split(/(\*\*[^*]+\*\*)/g);
  return parts.map((part, index) =>
    part.startsWith("**") && part.endsWith("**") ? (
      <strong key={index}>{part.slice(2, -2)}</strong>
    ) : (
      part
    )
  );
}

function ChangelogBlock({ text }: { text: string }) {
  const lines = text.split("\n");
  const blocks: React.ReactNode[] = [];
  let listItems: string[] = [];

  const flushList = () => {
    if (listItems.length === 0) return;
    blocks.push(
      <ul key={`list-${blocks.length}`}>
        {listItems.map((item, i) => (
          <li key={i}>{renderInline(item)}</li>
        ))}
      </ul>
    );
    listItems = [];
  };

  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed) {
      flushList();
      continue;
    }
    if (trimmed === "---") {
      flushList();
      blocks.push(<hr key={`hr-${blocks.length}`} />);
      continue;
    }
    if (trimmed.startsWith("## ")) {
      flushList();
      blocks.push(
        <h4 key={`h4-${blocks.length}`} className="changelog-version">
          {trimmed.slice(3)}
        </h4>
      );
      continue;
    }
    if (trimmed.startsWith("### ")) {
      flushList();
      blocks.push(
        <h5 key={`h5-${blocks.length}`} className="changelog-heading">
          {trimmed.slice(4)}
        </h5>
      );
      continue;
    }
    if (trimmed.startsWith("- ")) {
      listItems.push(trimmed.slice(2));
      continue;
    }
    flushList();
    blocks.push(
      <p key={`p-${blocks.length}`} className="muted">
        {renderInline(trimmed)}
      </p>
    );
  }
  flushList();

  return <>{blocks}</>;
}

export default function ChangelogPanel() {
  return (
    <section className="changelog-panel">
      <h3>Changelog</h3>
      <div className="changelog-scroll">
        <ChangelogBlock text={changelogBody()} />
      </div>
    </section>
  );
}
