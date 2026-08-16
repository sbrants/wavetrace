import { useEffect, useState } from "react";
import { api, Account, AccountList } from "../api";
import { confirmDialog } from "../confirmDialog";
import { reportUiError } from "../uiError";
import { showToast } from "../toast";

function emptyList(): AccountList {
  return { accounts: [], current_id: "", colors: [] };
}

export default function AccountsSettings({
  scannerRunning,
}: {
  scannerRunning: boolean;
}) {
  const [list, setList] = useState<AccountList>(emptyList);
  const [newName, setNewName] = useState("");
  const [newColor, setNewColor] = useState("#34d399");

  const reload = () => api.listAccounts().then(setList).catch(() => {});

  useEffect(() => {
    reload();
  }, []);

  useEffect(() => {
    const unused = list.colors.find(
      (c) => !list.accounts.some((a) => a.color.toLowerCase() === c.toLowerCase())
    );
    if (unused) setNewColor(unused);
  }, [list]);

  const current = list.accounts.find((a) => a.id === list.current_id);

  const add = () => {
    const name = newName.trim();
    if (!name) return;
    api
      .createAccount(name, newColor)
      .then((account) => {
        setNewName("");
        showToast(`Added ${account.name}`);
        return reload();
      })
      .catch((e) => reportUiError(e, "AccountsSettings.create"));
  };

  const rename = (account: Account, name: string) => {
    const trimmed = name.trim();
    if (!trimmed || trimmed === account.name) return;
    api
      .updateAccount(account.id, trimmed, null)
      .then(() => reload())
      .catch((e) => reportUiError(e, "AccountsSettings.rename"));
  };

  const recolor = (account: Account, color: string) => {
    api
      .updateAccount(account.id, null, color)
      .then(() => {
        void reload();
        if (account.id === list.current_id) {
          document.documentElement.style.setProperty("--account-color", color);
        }
      })
      .catch((e) => reportUiError(e, "AccountsSettings.color"));
  };

  const remove = async (account: Account) => {
    const ok = await confirmDialog({
      title: `Delete ${account.name}?`,
      message:
        "This account's runs, settings, and logs are removed. Other accounts are left alone.",
      confirmLabel: "Delete account",
      danger: true,
    });
    if (!ok) return;
    api
      .deleteAccount(account.id)
      .then(() => {
        showToast(`Deleted ${account.name}`);
        return reload();
      })
      .catch((e) => reportUiError(e, "AccountsSettings.delete"));
  };

  return (
    <section>
      <h3>Accounts</h3>
      <p className="muted">
        Each account has its own settings, target window, and run history. Open
        another window to monitor two logins at once — pick a color so you can
        tell them apart.
      </p>
      <ul className="account-settings-list">
        {list.accounts.map((account) => {
          const active = account.id === list.current_id;
          return (
            <li key={account.id} className="account-settings-row">
              <input
                type="color"
                value={account.color}
                aria-label={`${account.name} color`}
                onChange={(e) => recolor(account, e.target.value)}
              />
              <input
                type="text"
                defaultValue={account.name}
                aria-label="Account name"
                maxLength={40}
                onBlur={(e) => rename(account, e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    (e.target as HTMLInputElement).blur();
                  }
                }}
              />
              {active ? (
                <span className="account-this">this window</span>
              ) : (
                <>
                  <button
                    type="button"
                    disabled={scannerRunning}
                    title={
                      scannerRunning
                        ? "Stop the scanner before switching this window"
                        : "Use this account in this window"
                    }
                    onClick={() =>
                      api
                        .switchAccount(account.id)
                        .catch((e) => reportUiError(e, "AccountsSettings.switch"))
                    }
                  >
                    Switch
                  </button>
                  <button
                    type="button"
                    onClick={() =>
                      api
                        .openAccountWindow(account.id)
                        .then(() =>
                          showToast(`Opened ${account.name} in a new window`)
                        )
                        .catch((e) =>
                          reportUiError(e, "AccountsSettings.openWindow")
                        )
                    }
                  >
                    New window
                  </button>
                  <button
                    type="button"
                    className="danger"
                    disabled={list.accounts.length < 2}
                    onClick={() => void remove(account)}
                  >
                    Delete
                  </button>
                </>
              )}
            </li>
          );
        })}
      </ul>
      <div className="row account-add-row">
        <input
          type="color"
          value={newColor}
          aria-label="New account color"
          onChange={(e) => setNewColor(e.target.value)}
        />
        <input
          type="text"
          placeholder="New account name"
          value={newName}
          maxLength={40}
          onChange={(e) => setNewName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") add();
          }}
        />
        <button type="button" className="primary" onClick={add} disabled={!newName.trim()}>
          Add account
        </button>
      </div>
      {current && (
        <p className="muted">
          This window is <strong>{current.name}</strong>. Its data folder is listed
          under Data location below.
        </p>
      )}
    </section>
  );
}
