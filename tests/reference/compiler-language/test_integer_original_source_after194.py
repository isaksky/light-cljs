"""Exact original source-site contract guards; execution lives in real raw proof."""
import copy,unittest
import integer_original_source_after194 as proof
class WholeIntegerOriginal(unittest.TestCase):
    def test_exact_whole_original_and_only_one_site(self):
        binding=proof.binding()
        self.assertEqual(binding['complete_original_body']['sha256'],proof.BODY_HASH)
        self.assertEqual(binding['complete_original_setup']['sha256'],proof.TABLE_HASH)
        self.assertEqual(len(binding['proposed_original_assertion_witnesses']),1)
        self.assertEqual(binding['new_public_or_local_arity_links'],0)
        self.assertEqual(binding['installed_source_credit'],0)
    def test_literal_outcomes_and_messages_cover_all112(self):
        packet=proof.expected_local_packet();checks=packet['outcome']['value']['$vector'][1]['$vector']
        self.assertEqual(len(checks),112)
        self.assertEqual([r['$vector'][0] for r in checks],list(range(8))*14)
        self.assertTrue(all(r['$vector'][1] is True for r in checks))
        self.assertEqual(packet['trace'],[]);self.assertEqual(packet['stdout'],'')
    def test_literal_packet_does_not_allow_effect_stdout_exception_or_weakened_assertion(self):
        expected=proof.expected_local_packet()
        mutations=[]
        for field,value in [('trace',[1]),('stdout','hidden output'),('outcome',{'status':'threw','error':{'category':'error'}})]:
            changed=copy.deepcopy(expected);changed[field]=value;mutations.append(changed)
        changed=copy.deepcopy(expected);changed['outcome']['value']['$vector'][1]['$vector'][0]['$vector'][1]=False;mutations.append(changed)
        changed=copy.deepcopy(expected);changed['outcome']['value']['$vector'][1]['$vector'].pop();mutations.append(changed)
        for changed in mutations:
            with self.assertRaises(ValueError):proof.same(changed,expected,'Complete literal drift')
if __name__=='__main__':unittest.main()
