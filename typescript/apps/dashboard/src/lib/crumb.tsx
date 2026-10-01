import { createContext, useContext, useEffect } from "react";

/** Pages name the last breadcrumb (a market's name) for the layout's header. */
export const CrumbContext = createContext<(crumb: string | null) => void>(() => {});

export function useCrumb(crumb: string | null) {
  const set = useContext(CrumbContext);
  useEffect(() => {
    set(crumb);
    return () => set(null);
  }, [crumb, set]);
}
