'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { FileText, CalendarDays } from 'lucide-react';

interface MeetingMeta {
  created_at: string;
  updated_at: string;
}

/** "42 min" / "1 h 10 min" from created→updated span; null when implausible
 * (e.g. inflated by a much-later rename) so we show nothing rather than nonsense. */
function formatDuration(meta: MeetingMeta | undefined): string | null {
  if (!meta) return null;
  const ms = new Date(meta.updated_at).getTime() - new Date(meta.created_at).getTime();
  const min = Math.round(ms / 60000);
  if (!Number.isFinite(min) || min < 1 || min > 8 * 60) return null;
  if (min < 60) return `${min} min`;
  const h = Math.floor(min / 60);
  const rem = min % 60;
  return rem === 0 ? `${h} h` : `${h} h ${rem} min`;
}

/** "Today", "Yesterday", or "Jun 12" from the recording start. */
function formatWhen(meta: MeetingMeta | undefined): string | null {
  if (!meta) return null;
  const d = new Date(meta.created_at);
  if (isNaN(d.getTime())) return null;
  const now = new Date();
  const startOfDay = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const dayDiff = Math.round((startOfDay(now) - startOfDay(d)) / 86400000);
  if (dayDiff === 0) return 'Today';
  if (dayDiff === 1) return 'Yesterday';
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

/**
 * Centura Capture Home dashboard — shown on the home screen when not recording and
 * there are no live transcripts. Hero + recent meetings + (stubbed) today's calendar.
 */
export function HomeDashboard() {
  const router = useRouter();
  const { meetings, setCurrentMeeting } = useSidebar();
  const [metaById, setMetaById] = useState<Record<string, MeetingMeta>>({});

  // Most recent first; skip the placeholder "new call" entry.
  const recent = [...(meetings ?? [])]
    .filter((m) => m.id && m.id !== 'intro-call')
    .slice(-6)
    .reverse();

  // Fetch date + duration metadata for the visible cards (best-effort).
  const recentIds = recent.map((m) => m.id).join(',');
  useEffect(() => {
    let cancelled = false;
    const ids = recentIds ? recentIds.split(',') : [];
    if (ids.length === 0) return;
    (async () => {
      const entries = await Promise.all(
        ids.map(async (id) => {
          try {
            const meta = await invoke<MeetingMeta>('api_get_meeting_metadata', { meetingId: id });
            return [id, meta] as const;
          } catch {
            return null;
          }
        })
      );
      if (cancelled) return;
      const next: Record<string, MeetingMeta> = {};
      for (const e of entries) {
        if (e) next[e[0]] = e[1];
      }
      setMetaById(next);
    })();
    return () => {
      cancelled = true;
    };
  }, [recentIds]);

  const openMeeting = (m: { id: string; title: string }) => {
    setCurrentMeeting({ id: m.id, title: m.title });
    router.push(`/meeting-details?id=${m.id}`);
  };

  return (
    <div className="max-w-3xl mx-auto w-full px-6 py-10">
      {/* Hero */}
      <div className="text-center mb-10">
        <h1 className="text-2xl font-semibold text-gray-800">Ready to capture</h1>
        <p className="text-sm mt-1 text-gray-600">
          Press the <span className="font-medium text-centura-saffron">gold record button</span> below to start live transcription.
        </p>
        <p className="text-xs mt-3 text-gray-400">
          Audio is never saved — only the transcript. Notes are written to your CenturaOS vault.
        </p>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        {/* Recent meetings */}
        <div className="rounded-lg border border-border bg-card p-4 shadow-sm">
          <div className="flex items-center gap-2 mb-3">
            <FileText className="w-4 h-4 text-centura-blue" />
            <h2 className="text-sm font-semibold text-centura-blue">Recent meetings</h2>
          </div>
          {recent.length === 0 ? (
            <p className="text-sm text-gray-400">No meetings yet — your captured meetings will appear here.</p>
          ) : (
            <ul className="space-y-1">
              {recent.map((m) => {
                const when = formatWhen(metaById[m.id]);
                const duration = formatDuration(metaById[m.id]);
                const detail = [when, duration].filter(Boolean).join(' · ');
                return (
                  <li key={m.id}>
                    <button
                      onClick={() => openMeeting(m)}
                      className="w-full text-left px-3 py-2 rounded-md text-sm text-gray-700 hover:bg-centura-blue/10 hover:text-centura-blue transition-colors"
                      title={m.title}
                    >
                      <span className="block truncate">{m.title}</span>
                      {detail && <span className="block text-xs text-gray-400 mt-0.5">{detail}</span>}
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </div>

        {/* Today's calendar (stubbed until the Outlook connector is wired) */}
        <div className="rounded-lg border border-border bg-card p-4 shadow-sm">
          <div className="flex items-center gap-2 mb-3">
            <CalendarDays className="w-4 h-4 text-centura-blue" />
            <h2 className="text-sm font-semibold text-centura-blue">Today&apos;s meetings</h2>
          </div>
          <p className="text-sm text-gray-400">
            Calendar not connected yet. Once your Outlook calendar is linked, today&apos;s meetings will appear here so you can capture the right one.
          </p>
        </div>
      </div>
    </div>
  );
}
