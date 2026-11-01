-- Old provider settings as the pre-workspace app stored them (keys in plain
-- text). Used by the migration test; every key must end up in the key store.
INSERT INTO settings (id, provider, model, whisperModel, groqApiKey, openaiApiKey, anthropicApiKey,
                      ollamaApiKey, openRouterApiKey, ollamaEndpoint, customOpenAIConfig, geminiApiKey)
VALUES ('1', 'openai', 'gpt-4o-2024-11-20', 'large-v3', 'gsk-fixture-groq', 'sk-fixture-openai',
        'sk-ant-fixture-claude', 'ol-fixture-ollama', 'sk-or-fixture-router', 'http://gpu-box:11434',
        '{"endpoint":"http://localhost:8000/v1","apiKey":"cu-fixture-custom","model":"llama-3-70b","maxTokens":2048,"temperature":0.2,"topP":0.9}',
        'gm-fixture-gemini');

INSERT INTO transcript_settings (id, provider, model, whisperApiKey, deepgramApiKey, elevenLabsApiKey,
                                 groqApiKey, openaiApiKey)
VALUES ('1', 'openai', 'whisper-1', NULL, 'dg-fixture-deepgram', NULL, 'gsk-fixture-groq',
        'sk-fixture-openai-transcription');
