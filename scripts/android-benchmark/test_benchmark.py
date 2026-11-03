import copy
import unittest
import benchmark as b


class BenchmarkTests(unittest.TestCase):
    def fixture(self):
        clips = [dict(id=k, kind=k, reference='Århus Tivoli', sha256='a'*64,
                      duration_s=15) for k in ('quiet', 'noise')]
        clips.append(dict(id='silence', kind='silence', reference='',
                          sha256='b'*64, duration_s=15))
        data = dict(consent='explicit test recording consent', clips=clips,
                    engine_revision='c'*40, runs=[])
        for tier in ('midrange', 'recent'):
            for model in ('base', 'small', 'small-q5_1'):
                for clip in clips:
                    for repeat in range(20):
                        data['runs'].append(dict(tier=tier, hardware='test phone',
                            os='Android test', abi='arm64-v8a', model=model,
                            model_sha256='d'*64, model_bytes=100, clip=clip['id'],
                            repeat=repeat, hypothesis=clip['reference'], cold_ms=500,
                            warm_ms=1000, peak_pss_mb=400, energy_mwh=2,
                            temperature_start_c=25, temperature_end_c=26,
                            thermal_status=0))
        return data

    def test_normalization_and_edits(self):
        self.assertEqual(b.tokens('ÅRHUS, Tivoli!'), ['århus', 'tivoli'])
        self.assertEqual(b.distance(['a', 'b'], ['a', 'c', 'd']), 2)

    def test_complete_matrix_and_numeric_only_report(self):
        result = b.summarize(self.fixture())
        self.assertEqual(len(result), 6)
        self.assertTrue(all(x['accepted'] for x in result))
        self.assertNotIn('Tivoli', str(result))

    def test_thresholds_and_silence_are_real_gates(self):
        data = self.fixture()
        for row in data['runs']:
            row.update(warm_ms=3001, peak_pss_mb=701, hypothesis='forkert tekst')
        result = b.summarize(data)
        self.assertTrue(all(not x['accepted'] for x in result))
        self.assertGreater(result[0]['wer_quiet'], .15)
        self.assertGreater(result[0]['silence_outputs'], 0)

    def test_incomplete_and_invalid_fail_closed(self):
        for mutate in (lambda d: d['runs'].pop(),
                       lambda d: d['runs'][0].update(warm_ms=float('nan')),
                       lambda d: d['runs'][0].update(abi='x86_64'),
                       lambda d: d.update(consent=''),
                       lambda d: d['runs'][0].update(model_sha256='unknown')):
            data = copy.deepcopy(self.fixture())
            mutate(data)
            with self.assertRaises(ValueError):
                b.summarize(data)

    def test_nearest_rank_p95(self):
        self.assertEqual(b.percentile(list(range(1, 21)), .95), 19)


if __name__ == '__main__':
    unittest.main()
