import type { AnchorHTMLAttributes, MouseEvent, ReactNode } from "react";
import { api } from "./api";

type Props = Omit<AnchorHTMLAttributes<HTMLAnchorElement>, "href" | "onClick"> & {
  href: string;
  children: ReactNode;
};

/** Open http(s) links via the OS handler (Tauri webview blocks normal navigation). */
export default function ExternalLink({ href, children, ...rest }: Props) {
  const onClick = (e: MouseEvent<HTMLAnchorElement>) => {
    e.preventDefault();
    void api.openExternalUrl(href);
  };
  return (
    <a href={href} onClick={onClick} {...rest}>
      {children}
    </a>
  );
}
