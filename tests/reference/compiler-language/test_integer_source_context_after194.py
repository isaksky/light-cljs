"""Failclosed exact original-source mapping; real packets require executable proof."""
import copy,unittest
import integer_source_context_after194 as adapter
class IntegerSourceContext(unittest.TestCase):
    def test_mapping_is_only_original_site_without_arity_links(self):
        mapping=adapter.mapping()
        self.assertEqual(mapping['assertion_id'],adapter.SITE)
        self.assertEqual(mapping['assertion_sha256'],adapter.whole.SITE_HASH)
        self.assertEqual(mapping['new_public_or_local_arity_links'],0)
    def test_registration_without_whole_source_execution_rejects(self):
        value={'profile':adapter.PROFILE,'mapping':adapter.mapping(),'current_binding':adapter.current_binding(),'strict_complete':False}
        with self.assertRaises(ValueError):adapter.precheck(value)
    def test_original_source_mapping_and_current_binding_tamper_rejects(self):
        value={'profile':adapter.PROFILE,'mapping':adapter.mapping(),'current_binding':adapter.current_binding(),'strict_complete':False}
        for name,changed in [('profile','wrong'),('mapping',{}),('current_binding',{})]:
            document=copy.deepcopy(value);document[name]=changed
            with self.assertRaises(ValueError):adapter.precheck(document)
if __name__=='__main__':unittest.main()
