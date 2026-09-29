import React, { useState, useEffect } from "react";
import { getVersion } from '@tauri-apps/api/app';
import Image from 'next/image';
import AnalyticsConsentSwitch from "./AnalyticsConsentSwitch";

// Centura Capture: the "Check for Updates" button and UpdateDialog were removed on purpose.
// The Meetily updater pointed at the stock Meetily release feed and replaced the fork with
// stock Meetily (no Deepgram, no vault export). Centura Capture ships through Intune instead.
// The copy below must stay accurate about data flow: audio goes to Deepgram's cloud.

function FlowCard({ title, children }: { title: string; children: React.ReactNode }) {
    return (
        <div className="bg-centura-gainsboro/30 rounded p-3">
            <h3 className="font-semibold text-sm text-centura-blue mb-1">{title}</h3>
            <p className="text-xs text-gray-600 leading-relaxed">{children}</p>
        </div>
    );
}

export function About() {
    const [currentVersion, setCurrentVersion] = useState<string>('0.4.1');

    useEffect(() => {
        getVersion().then(setCurrentVersion).catch(console.error);
    }, []);

    return (
        <div className="p-4 space-y-4 h-[80vh] overflow-y-auto">
            <div className="text-center">
                <div className="mb-3">
                    <Image
                        src="icon_128x128.png"
                        alt="Centura Capture"
                        width={64}
                        height={64}
                        className="mx-auto"
                    />
                </div>
                <h1 className="text-xl font-semibold text-centura-blue">Centura Capture</h1>
                <span className="text-sm text-gray-500">v{currentVersion}</span>
                <p className="text-medium text-gray-600 mt-1">
                    Live meeting transcription for Centura Wealth Advisory.
                </p>
                <p className="mt-2 text-xs text-gray-500">
                    Centura IT distributes Centura Capture updates. This app never updates itself.
                </p>
            </div>

            <div className="space-y-3">
                <h2 className="text-base font-semibold text-gray-800">How your meeting data flows</h2>
                <div className="grid grid-cols-2 gap-2">
                    <FlowCard title="Transcription">
                        Meeting audio streams to Deepgram for transcription, with no-retention requested.
                        If Deepgram is unavailable, the app transcribes on this computer instead.
                    </FlowCard>
                    <FlowCard title="No saved audio">
                        The app never writes meeting audio to disk. It keeps only the text transcript.
                    </FlowCard>
                    <FlowCard title="Vault export">
                        When a meeting ends, the transcript goes to your CenturaOS vault Inbox for
                        synthesis into meeting notes.
                    </FlowCard>
                    <FlowCard title="Local records">
                        Transcripts and meeting history stay on this computer.
                    </FlowCard>
                </div>
            </div>

            <div className="pt-2 border-t border-gray-200 text-center">
                <p className="text-xs text-gray-400">
                    Built on the open-source Meetily project by Zackriya Solutions (MIT License).
                </p>
            </div>
            <AnalyticsConsentSwitch />
        </div>
    )
}
