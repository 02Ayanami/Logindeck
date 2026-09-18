import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, expect, it, vi } from 'vitest';
const { get, start, cancel, confirm } = vi.hoisted(() => ({ get: vi.fn(), start: vi.fn(), cancel: vi.fn(), confirm: vi.fn() }));
vi.mock('../../lib/tauri', () => ({ tauri: { getScanStatus: get, rescanApplications: start, cancelApplicationScan: cancel, confirmApplicationScan: confirm } }));
import { useApplicationScan } from './useApplicationScan';
const idle = { id: 0, phase: 'idle', started_at: null, count: null, error: null };
const running = { ...idle, id: 1, phase: 'scanning', started_at: Date.now() };
beforeEach(() => { vi.resetAllMocks(); get.mockResolvedValue(idle); });
it('rejects an old poll after a newer start and deduplicates rapid starts', async () => {
 let release!: (x: unknown) => void;
 get.mockReturnValue(new Promise(resolve => { release = resolve; }));
 start.mockResolvedValue(running);
 const { result } = renderHook(() => useApplicationScan(vi.fn()));
 await act(async () => { await Promise.all([result.current.start(), result.current.start()]); });
 await act(async () => { release(idle); });
 expect(result.current.status?.phase).toBe('scanning');
 expect(start).toHaveBeenCalledOnce();
});
it('recovers a backend task after remount and cancels by its id', async () => {
 get.mockResolvedValue(running); cancel.mockResolvedValue({ ...running, phase: 'cancelled' });
 const first = renderHook(() => useApplicationScan(vi.fn()));
 await waitFor(() => expect(first.result.current.active).toBe(true)); first.unmount();
 const second = renderHook(() => useApplicationScan(vi.fn()));
 await waitFor(() => expect(second.result.current.active).toBe(true));
 await act(async () => { await second.result.current.cancel(); });
 expect(cancel).toHaveBeenCalledWith(1); expect(second.result.current.active).toBe(false);
 expect(start).not.toHaveBeenCalled();
});
it('ignores the abandoned StrictMode poll and refreshes a completed scan only once', async () => {
 let release!: (x: unknown) => void;
 get.mockReturnValueOnce(new Promise(resolve => { release = resolve; })).mockResolvedValue({ ...running, phase: 'completed', count: 2 });
 const refreshed = vi.fn().mockResolvedValue(undefined);
 const { result } = renderHook(() => useApplicationScan(refreshed), { reactStrictMode: true });
 await waitFor(() => expect(refreshed).toHaveBeenCalledOnce());
 await act(async () => { release({ ...running, id: 99 }); });
 expect(result.current.status?.phase).toBe('completed');
 expect(refreshed).toHaveBeenCalledOnce();
});
it('retains a start error when a normal idle poll arrives', async () => {
 let release!: (x: unknown) => void;
 get.mockReturnValue(new Promise(resolve => { release = resolve; }));
 const failure = new Error('unavailable'); start.mockRejectedValue(failure);
 const { result } = renderHook(() => useApplicationScan(vi.fn()));
 await act(async () => { await result.current.start(); });
 await act(async () => { release(idle); });
 expect(result.current.error).toBe(failure);
});

it('holds discovered candidates for explicit confirmation and ignores a late choosing poll', async () => {
 const choosing = { ...running, phase: 'choosing', candidates: [] };
 get.mockResolvedValue(choosing);
 const refreshed = vi.fn().mockResolvedValue(undefined);
 let release!: (value: unknown) => void;
 confirm.mockReturnValue(new Promise(resolve => { release = resolve; }));
 const { result } = renderHook(() => useApplicationScan(refreshed));
 await waitFor(() => expect(result.current.status?.phase).toBe('choosing'));
 expect(refreshed).not.toHaveBeenCalled();
 let first!: Promise<void>;
 act(() => { first = result.current.confirm([]); void result.current.confirm([]); });
 expect(confirm).toHaveBeenCalledOnce();
 expect(confirm).toHaveBeenCalledWith(1, []);
 await act(async () => { release({ ...choosing, phase: 'committing' }); await first; });
 expect(result.current.status?.phase).toBe('committing');
 await new Promise(resolve => setTimeout(resolve, 800));
 expect(result.current.status?.phase).toBe('committing');
 expect(refreshed).not.toHaveBeenCalled();
});
it('cancels the selection phase instead of saving discovery automatically', async () => {
 get.mockResolvedValue({...running,phase:'choosing',candidates:[]});
 cancel.mockResolvedValue({...running,phase:'cancelled'});
 const refreshed = vi.fn();
 const {result} = renderHook(()=>useApplicationScan(refreshed));
 await waitFor(()=>expect(result.current.status?.phase).toBe('choosing'));
 await act(async()=>{await result.current.cancel();});
 expect(cancel).toHaveBeenCalledWith(1); expect(confirm).not.toHaveBeenCalled(); expect(refreshed).not.toHaveBeenCalled();
});
