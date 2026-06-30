import { useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

export interface TranscriptModelProps {
    provider: 'localWhisper' | 'parakeet' | 'deepgram' | 'elevenLabs' | 'groq' | 'openai';
    model: string;
    apiKey?: string | null;
}

export interface TranscriptSettingsProps {
    transcriptModelConfig: TranscriptModelProps;
    setTranscriptModelConfig: (config: TranscriptModelProps) => void;
    onModelSelect?: () => void;
}

// Centura Capture: transcription is Deepgram-only in the UI. The local engines
// (Parakeet/Whisper) remain as an automatic fallback in the backend if Deepgram is
// unavailable, but there's nothing to configure here — the API key comes from .env.
export function TranscriptSettings({ transcriptModelConfig, setTranscriptModelConfig }: TranscriptSettingsProps) {
    // Ensure the stored provider is Deepgram (no model-manager UI to set it anymore).
    useEffect(() => {
        if (transcriptModelConfig.provider !== 'deepgram') {
            const cfg: TranscriptModelProps = { provider: 'deepgram', model: 'nova-3', apiKey: null };
            setTranscriptModelConfig(cfg);
            invoke('api_save_transcript_config', { provider: 'deepgram', model: 'nova-3', apiKey: null })
                .catch((e) => console.error('Failed to set Deepgram as transcript provider', e));
        }
    }, [transcriptModelConfig.provider, setTranscriptModelConfig]);

    return (
        <div className="space-y-4 pt-8 border-t border-gray-200">
            <div>
                <h3 className="text-lg font-semibold mb-1">Transcription</h3>
                <p className="text-sm text-gray-600">
                    How meeting audio is turned into text.
                </p>
            </div>

            <div className="rounded-lg border border-gray-200 p-4 bg-centura-gainsboro/30">
                <div className="flex items-center gap-2">
                    <span className="inline-block w-2 h-2 rounded-full bg-centura-green" />
                    <span className="font-medium text-centura-blue">Deepgram — cloud streaming + diarization</span>
                </div>
                <dl className="mt-3 text-sm text-gray-700 space-y-1">
                    <div className="flex gap-2">
                        <dt className="text-gray-500 w-24">Model</dt>
                        <dd>nova-3 (English)</dd>
                    </div>
                    <div className="flex gap-2">
                        <dt className="text-gray-500 w-24">API key</dt>
                        <dd>Loaded from local <code>.env</code> (not stored in the app)</dd>
                    </div>
                    <div className="flex gap-2">
                        <dt className="text-gray-500 w-24">Privacy</dt>
                        <dd>No-retention requested; audio is never saved to disk</dd>
                    </div>
                    <div className="flex gap-2">
                        <dt className="text-gray-500 w-24">Fallback</dt>
                        <dd>Falls back to on-device transcription automatically if Deepgram is unavailable</dd>
                    </div>
                </dl>
            </div>
        </div>
    );
}
