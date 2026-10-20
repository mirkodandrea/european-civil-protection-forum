import importlib.util
from pathlib import Path
import unittest
spec = importlib.util.spec_from_file_location('ttx_snapshot', Path(__file__).parents[1] / 'snapshot.py')
harness = importlib.util.module_from_spec(spec)
spec.loader.exec_module(harness)

class SnapshotSpecTests(unittest.TestCase):
    def test_earlier_time_and_independent_camera_states_are_retained(self):
        states = harness.parse('name=fire,t=30,at=fire,pitch=-35,ui=0;name=before,t=0,at=1600:1300')
        self.assertEqual(states[1]['t'], '0')
        self.assertEqual(states[0]['ui'], '0')
        self.assertEqual(states[1]['ui'], '1')
        self.assertEqual(states[1]['at'], '1600:1300')

    def test_unknown_keys_and_invalid_numbers_are_rejected(self):
        for text in ['unknown=1', 'dist=nan', 'yaw=inf', 'pitch=0', 't=-1', 't=0.5', 'ui=maybe', 'bench=-1', 'quality=ultra']:
            with self.subTest(text=text), self.assertRaises(ValueError):
                harness.parse(text)

    def test_names_cannot_traverse_paths_or_overwrite_each_other(self):
        for text in ['name=../oops', 'name=/tmp/oops', 'name=a;name=a']:
            with self.subTest(text=text), self.assertRaises(ValueError):
                harness.parse(text)

    def test_native_place_and_task_targets_are_supported(self):
        states = harness.parse('at=place:Pineta Primary School;at=task:12')
        self.assertEqual(states[1]['at'], 'task:12')
        with self.assertRaises(ValueError):
            harness.parse('at=task:bad')

if __name__ == '__main__':
    unittest.main()
