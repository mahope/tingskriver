import os
import tempfile
import unittest
from unittest.mock import AsyncMock, patch

from pydantic_ai.models.test import TestModel


class TranscriptProcessorTests(unittest.IsolatedAsyncioTestCase):
    @classmethod
    def setUpClass(cls):
        database = cls.enterClassContext(tempfile.TemporaryDirectory())
        cls.enterClassContext(
            patch.dict(os.environ, {"DATABASE_PATH": os.path.join(database, "test.db")})
        )
        import transcript_processor

        cls.module = transcript_processor

    async def test_returns_structured_summary_from_agent(self):
        processor = self.module.TranscriptProcessor()
        with (
            patch.object(self.module.db, "get_api_key", AsyncMock(return_value="test-key")),
            patch.object(self.module, "OpenAIChatModel", return_value=TestModel()),
        ):
            chunks, summaries = await processor.process_transcript(
                "The team agreed to meet on Monday.", "openai", "test-model"
            )

        self.assertEqual(chunks, 1)
        self.assertEqual(len(summaries), 1)
        self.module.SummaryResponse.model_validate_json(summaries[0])

    async def test_ollama_uses_local_client_without_creating_an_agent(self):
        processor = self.module.TranscriptProcessor()
        section = {"title": "Test", "blocks": []}
        summary = self.module.SummaryResponse(
            MeetingName="Test meeting",
            People=section,
            SessionSummary=section,
            CriticalDeadlines=section,
            KeyItemsDecisions=section,
            ImmediateActionItems=section,
            NextSteps=section,
            MeetingNotes={"meeting_name": "Test meeting", "sections": []},
        )
        local_chat = AsyncMock(return_value=summary)
        with (
            patch.object(processor, "chat_ollama_model", local_chat),
            patch.object(self.module, "Agent") as agent,
            patch.object(self.module, "OpenAIChatModel") as openai_model,
        ):
            chunks, summaries = await processor.process_transcript(
                "The team agreed to meet on Monday.", "ollama", "llama-test"
            )

        self.assertEqual((chunks, summaries), (1, [summary.model_dump_json()]))
        local_chat.assert_awaited_once_with(
            "llama-test", "The team agreed to meet on Monday.", ""
        )
        agent.assert_not_called()
        openai_model.assert_not_called()
