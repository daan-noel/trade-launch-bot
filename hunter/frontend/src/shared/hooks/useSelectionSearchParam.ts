import { useCallback, useEffect, useState } from 'react';
import { useSearchParams } from 'react-router-dom';

/**
 * Bidirectional sync between a table selection and one URL search param
 * (Tokens `?mint=`, Rules `?rule=`, Fingerprints `?fp=`).
 *
 * - Initial load / browser back-forward: URL seeds selection.
 * - User select/deselect: writes the param with `replace: true`.
 */
export function useSelectionSearchParam(param: string): [
  string | null,
  (key: string | null) => void,
] {
  const [searchParams, setSearchParams] = useSearchParams();
  const fromUrl = searchParams.get(param);
  const [selectedKey, setSelectedKey] = useState<string | null>(fromUrl);

  useEffect(() => {
    setSelectedKey(fromUrl);
  }, [fromUrl]);

  const select = useCallback(
    (key: string | null) => {
      setSelectedKey(key);
      setSearchParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          if (key) next.set(param, key);
          else next.delete(param);
          return next;
        },
        { replace: true },
      );
    },
    [param, setSearchParams],
  );

  return [selectedKey, select];
}
