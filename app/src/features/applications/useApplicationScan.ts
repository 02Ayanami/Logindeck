import { useCallback, useEffect, useRef, useState } from 'react';
import { tauri, type ScanStatus } from '../../lib/tauri';
export const scanActive = (status?: ScanStatus) => !!status && ['scanning', 'choosing', 'cancelling', 'committing'].includes(status.phase);

export function useApplicationScan(onCompleted: () => Promise<void>) {
  const [status, setStatus] = useState<ScanStatus>();
  const [checking, setChecking] = useState(true);
  const [error, setError] = useState<unknown>();
  const [pending, setPending] = useState(false);
  const latest = useRef<ScanStatus | undefined>(undefined);
  const [elapsed, setElapsed] = useState(0);
  const completed = useRef<number | undefined>(undefined);
  const callback = useRef(onCompleted);
  callback.current = onCompleted;
  const alive = useRef(false);
  const flight = useRef(false);
  const accept = useCallback(async (value: ScanStatus) => {
    if (!alive.current) return;
    const rank = { idle: 0, scanning: 1, choosing: 2, cancelling: 3, committing: 3, completed: 4, cancelled: 4, failed: 4 };
    const previous = latest.current;
    if (previous && (value.id < previous.id || (value.id === previous.id && rank[value.phase] < rank[previous.phase]))) return;
    latest.current = value;
    setStatus(value);
    if ((value.phase === 'completed' || (value.phase === 'failed' && previous?.phase === 'committing')) && completed.current !== value.id) {
      completed.current = value.id;
      try { await callback.current(); }
      catch (reason) { if (alive.current) { completed.current = undefined; setError(reason); } }
    }
  }, []);
  useEffect(() => {
    alive.current = true;
    let live = true;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try { const value = await tauri.getScanStatus(); if (live) await accept(value); }
      catch (reason) { if (live) setError(reason); }
      finally {
        if (live) { setChecking(false); timer = setTimeout(() => void poll(), 750); }
      }
    };
    void poll();
    return () => { live = false; alive.current = false; clearTimeout(timer); };
  }, [accept]);
  useEffect(() => {
    const tick = () => setElapsed(status?.started_at ? Math.max(0, Math.floor((Date.now() - status.started_at) / 1000)) : 0);
    tick();
    if (!scanActive(status)) return;
    const timer = setInterval(tick, 1000);
    return () => clearInterval(timer);
  }, [status?.started_at, status?.phase]);
  const start = async () => {
    if (flight.current || scanActive(latest.current)) return;
    flight.current = true; setPending(true); setError(undefined);
    try { await accept(await tauri.rescanApplications()); }
    catch (reason) { if (alive.current) setError(reason); }
    finally { flight.current = false; if (alive.current) setPending(false); }
  };
  const cancel = async () => {
    if (!status || !['scanning', 'choosing'].includes(status.phase) || flight.current) return;
    flight.current = true; setPending(true); setError(undefined);
    try { await accept(await tauri.cancelApplicationScan(status.id)); }
    catch (reason) { if (alive.current) setError(reason); }
    finally { flight.current = false; if (alive.current) setPending(false); }
  };
  const confirm = async (tokens: number[]) => {
    if (!status || status.phase !== 'choosing' || flight.current) return;
    flight.current = true; setPending(true); setError(undefined);
    try { await accept(await tauri.confirmApplicationScan(status.id, tokens)); }
    catch (reason) { if (alive.current) setError(reason); }
    finally { flight.current = false; if (alive.current) setPending(false); }
  };
  return { confirm, status, checking: checking || pending, error, elapsed, start, cancel, active: scanActive(status) };
}
