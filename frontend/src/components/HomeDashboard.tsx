'use client';

import { useRouter } from 'next/navigation';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { FileText, CalendarDays } from 'lucide-react';

/**
 * Centura Capture Home dashboard — shown on the home screen when not recording and
 * there are no live transcripts. Hero + recent meetings + (stubbed) today's calendar.
 */
export function HomeDashboard() {
  const router = useRouter();
  const { meetings, setCurrentMeeting } = useSidebar();

  // Most recent first; skip the placeholder "new call" entry.
  const recent = [...(meetings ?? [])]
    .filter((m) => m.id && m.id !== 'intro-call')
    .slice(-6)
    .reverse();

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
              {recent.map((m) => (
                <li key={m.id}>
                  <button
                    onClick={() => openMeeting(m)}
                    className="w-full text-left px-3 py-2 rounded-md text-sm text-gray-700 hover:bg-centura-blue/10 hover:text-centura-blue transition-colors truncate"
                    title={m.title}
                  >
                    {m.title}
                  </button>
                </li>
              ))}
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
