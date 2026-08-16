import { useEffect, useRef, useState } from "react";
import { api, Account, AccountList } from "../api";
import { reportUiError } from "../uiError";
import { showToast } from "../toast";

function defaultList(): AccountList {
  return { accounts: [], current_id: "", colors: [] };
}

export default function AccountSelector({
  scannerRunning,
  onManage,
}: {
  scannerRunning: boolean;
  onManage: () => void;
}) {
  const [list, setList] = useState<AccountList>(defaultList);
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  const reload = () => {
    api.listAccounts().then(setList).catch(() => {});
  };

  useEffect(() => {
    reload();
  }, []);

  useEffect(() => {
    const current = list.accounts.find((a) => a.id === list.current_id);
    if (current?.color) {
      document.documentElement.style.setProperty("--account-color", current.color);
    }
  }, [list]);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);

  const current = list.accounts.find((a) => a.id === list.current_id);

  const switchTo = (account: Account) => {
    if (account.id === list.current_id) {
      setOpen(false);
      return;
    }
    api
      .switchAccount(account.id)
      .then(() => {
        showToast(`Switching to ${account.name}…`);
      })
      .catch((e) => reportUiError(e, "AccountSelector.switch"));
  };

  const openOther = (account: Account) => {
    api
      .openAccountWindow(account.id)
      .then(() => showToast(`Opened ${account.name} in a new window`))
      .catch((e) => reportUiError(e, "AccountSelector.openWindow"));
  };

  return (
    <div className="account-selector" ref={rootRef}>
      <button
        type="button"
        className="account-selector-toggle"
        aria-haspopup="listbox"
        aria-expanded={open}
        title="Switch account"
        onClick={() => setOpen((v) => !v)}
      >
        <span
          className="account-dot"
          style={{ background: current?.color ?? "#4cc2ff" }}
          aria-hidden="true"
        />
        <span className="account-selector-name">{current?.name ?? "Account"}</span>
      </button>
      {open && (
        <div className="account-menu" role="listbox">
          {list.accounts.map((account) => {
            const active = account.id === list.current_id;
            return (
              <div key={account.id} className="account-menu-row">
                <button
                  type="button"
                  className={active ? "account-menu-item current" : "account-menu-item"}
                  onClick={() => switchTo(account)}
                  disabled={scannerRunning && !active}
                  title={
                    scannerRunning && !active
                      ? "Stop the scanner before switching this window"
                      : `Use ${account.name} in this window`
                  }
                >
                  <span
                    className="account-dot"
                    style={{ background: account.color }}
                    aria-hidden="true"
                  />
                  <span>{account.name}</span>
                  {active && <span className="account-this">this window</span>}
                </button>
                {!active && (
                  <button
                    type="button"
                    className="account-open-other"
                    title={`Monitor ${account.name} in a new WaveTrace window`}
                    onClick={() => openOther(account)}
                  >
                    New window
                  </button>
                )}
              </div>
            );
          })}
          <button
            type="button"
            className="account-menu-manage"
            onClick={() => {
              setOpen(false);
              onManage();
            }}
          >
            Manage accounts…
          </button>
        </div>
      )}
    </div>
  );
}
