import json
from pathlib import Path
root=Path(__file__).resolve().parent.parent
slug={'type':'string','pattern':'^[a-z][a-z0-9_-]{1,63}$'}
nonempty={'type':'string','minLength':1,'maxLength':200}
source_ref={'type':'object','required':['catalog','metric'], 'properties':{'catalog':slug,'metric':slug},'additionalProperties':False}
rect={'oneOf':[
 {'type':'object','required':['mode','w','h'],'properties':{'mode':{'const':'auto'},'w':{'type':'integer','minimum':1,'maximum':72},'h':{'type':'integer','minimum':1,'maximum':64}},'additionalProperties':False},
 {'type':'object','required':['mode','x','y','w','h'],'properties':{'mode':{'const':'fixed'},'x':{'type':'integer','minimum':0,'maximum':71},'y':{'type':'integer','minimum':0,'maximum':63},'w':{'type':'integer','minimum':1,'maximum':72},'h':{'type':'integer','minimum':1,'maximum':64}},'additionalProperties':False}
]}
role_ref={'type':'object','required':['role','selection'],'properties':{'role':slug,'selection':{'enum':['all','one','slot']},'slot_id':slug},'allOf':[{'if':{'properties':{'selection':{'const':'slot'}},'required':['selection']},'then':{'required':['slot_id']},'else':{'not':{'required':['slot_id']}}}],'additionalProperties':False}
series={'type':'object','required':['metric','hosts'],'properties':{'metric':source_ref,'hosts':role_ref,'color':{'type':'string','pattern':'^#[0-9a-fA-F]{6}$'}},'additionalProperties':False}
graph={'type':'object','required':['id','type','title','expansion','series','layout'],'properties':{'id':slug,'type':{'const':'graph'},'title':nonempty,'expansion':{'const':'series_per_host'},'series':{'type':'array','minItems':1,'maxItems':32,'items':series},'layout':rect},'additionalProperties':False}
item={'type':'object','required':['id','type','title','expansion','metric','hosts','layout'],'properties':{'id':slug,'type':{'const':'item_value'},'title':nonempty,'expansion':{'enum':['single','widget_per_host']},'metric':source_ref,'hosts':role_ref,'layout':rect},'additionalProperties':False}
page={'type':'object','required':['id','name','widgets'],'properties':{'id':slug,'name':nonempty,'widgets':{'type':'array','maxItems':256,'items':{'oneOf':[graph,item]}}},'additionalProperties':False}
dash={'type':'object','required':['id','name','kind','pages'],'properties':{'id':slug,'name':nonempty,'kind':{'const':'global'},'pages':{'type':'array','minItems':1,'maxItems':32,'items':page}},'additionalProperties':False}
project={'$schema':'https://json-schema.org/draft/2020-12/schema','$id':'https://zdashstitch.example/schemas/project-v1.schema.json','title':'ZDashstitch portable project v1', 'type':'object','required':['format','project_id','catalogs','roles','dashboards'],'properties':{
 'format':{'const':'zdashstitch.project/v1'},'project_id':slug,
 'catalogs':{'type':'array','minItems':1,'maxItems':32,'items':{'type':'object','required':['id','file'],'properties':{'id':slug,'file':{'type':'string','pattern':'^metrics/[a-z][a-z0-9_-]{1,63}\\.ya?ml$'}},'additionalProperties':False}},
 'roles':{'type':'object','minProperties':1,'maxProperties':64,'patternProperties':{'^[a-z][a-z0-9_-]{1,63}$':{'type':'object','required':['cardinality'],'properties':{'cardinality':{'enum':['one','many']},'description':{'type':'string','maxLength':300},'slots':{'type':'array','minItems':1,'maxItems':256,'uniqueItems':True,'items':slug}},'additionalProperties':False}},'additionalProperties':False},
 'dashboards':{'type':'array','minItems':1,'maxItems':16,'items':dash}
 },'additionalProperties':False}
version_pattern = r'^[1-9][0-9]*\.[0-9]+(?:\.[0-9]+)?(?:[A-Za-z][A-Za-z0-9._-]*)?$'
version_range={'type':'object','required':['minimum'],'properties':{
 'minimum':{'type':'string','pattern':version_pattern},
 'maximum_exclusive':{'type':'string','pattern':version_pattern}
 },'additionalProperties':False}
provenance={'oneOf':[
 {'type':'object','required':['kind'],'properties':{'kind':{'const':'local'}},'additionalProperties':False},
 {'type':'object','required':['kind','legacy_format','source_project_id','source_dashboard_id'],'properties':{
  'kind':{'const':'migrated_v1'},'legacy_format':{'const':'zdashstitch.project/v1'},
  'source_project_id':slug,'source_dashboard_id':slug,'source_page_id':slug
 },'additionalProperties':False},
 {'type':'object','required':['kind'],'properties':{'kind':{'const':'server_snapshot'}},'additionalProperties':False}
]}
registry_state={'enum':['known','unknown']}
widget_object_kind={'enum':['host_group','host','item','item_prototype','graph','graph_prototype','map','service','sla','user','action','media_type','unknown']}
widget_field_value={'oneOf':[
 {'type':'object','required':['kind','value'],'properties':{'kind':{'const':'integer'},'value':{'type':'integer','minimum':-9223372036854775808,'maximum':9223372036854775807}},'additionalProperties':False},
 {'type':'object','required':['kind','value'],'properties':{'kind':{'const':'string'},'value':{'type':'string','maxLength':16384}},'additionalProperties':False},
 {'type':'object','required':['kind','role','selection'],'properties':{'kind':{'const':'host'},**role_ref['properties']},'allOf':role_ref['allOf'],'additionalProperties':False},
 {'type':'object','required':['kind','catalog','metric','hosts'],'properties':{'kind':{'const':'metric'},'catalog':slug,'metric':slug,'hosts':role_ref},'additionalProperties':False},
 {'type':'object','required':['kind','object_kind','display_name'],'properties':{'kind':{'const':'unresolved'},'object_kind':widget_object_kind,'display_name':{'type':'string','maxLength':500}},'additionalProperties':False}
]}
zabbix_field={'type':'object','required':['name','field_type','registry_state','value'],'properties':{
 'name':{'type':'string','minLength':1,'maxLength':255},'field_type':{'type':'integer','minimum':0,'maximum':65535},
 'registry_state':registry_state,'value':widget_field_value
},'allOf':[
 {'if':{'properties':{'field_type':{'minimum':14}},'required':['field_type']},'then':{
  'properties':{'registry_state':{'const':'unknown'},'value':{'properties':{'kind':{'const':'unresolved'},'object_kind':{'const':'unknown'}},'required':['kind','object_kind']}}
 }}
],'additionalProperties':False}
zabbix_widget={'type':'object','required':['id','type','widget_type','name','registry_state','fields','layout'],'properties':{
 'id':slug,'type':{'const':'zabbix'},'widget_type':{'type':'string','pattern':'^[a-z][a-z0-9._-]{0,127}$'},
 'name':{'type':'string','maxLength':200},'registry_state':registry_state,
 'fields':{'type':'array','maxItems':1024,'items':zabbix_field},'layout':rect
},'additionalProperties':False}
page_v2={'type':'object','required':['id','name','widgets'],'properties':{'id':slug,'name':nonempty,'widgets':{'type':'array','maxItems':256,'items':{'oneOf':[graph,item,zabbix_widget]}}},'additionalProperties':False}
dash_v2={'type':'object','required':['id','name','kind','pages'],'properties':{'id':slug,'name':nonempty,'kind':{'const':'global'},'pages':{'type':'array','minItems':1,'maxItems':32,'items':page_v2}},'additionalProperties':False}
project_v2={'$schema':'https://json-schema.org/draft/2020-12/schema','$id':'https://zdashstitch.example/schemas/project-v2.schema.json','title':'ZDashstitch typed portable project v2', 'type':'object',
 'required':['format','project_id','kind','declared_compatibility','provenance','catalogs','roles','root'],
 'properties':{
  'format':{'const':'zdashstitch.project/v2'},'project_id':slug,'kind':{'enum':['dashboard','page']},
  'declared_compatibility':version_range,'provenance':provenance,
  'catalogs':{**project['properties']['catalogs'],'minItems':0},
  'roles':{**project['properties']['roles'],'minProperties':0},
  'root':{'oneOf':[dash_v2,page_v2]}
 },
 'allOf':[
  {'if':{'properties':{'kind':{'const':'dashboard'}},'required':['kind']},'then':{'properties':{'root':dash_v2}}},
  {'if':{'properties':{'kind':{'const':'page'}},'required':['kind']},'then':{'properties':{'root':page_v2}}},
  {'if':{'properties':{'kind':{'const':'page'},'provenance':{'properties':{'kind':{'const':'migrated_v1'}},'required':['kind']}},'required':['kind','provenance']},'then':{'properties':{'provenance':{'required':['source_page_id']}}}},
  {'if':{'properties':{'kind':{'const':'dashboard'},'provenance':{'properties':{'kind':{'const':'migrated_v1'}},'required':['kind']}},'required':['kind','provenance']},'then':{'properties':{'provenance':{'not':{'required':['source_page_id']}}}}}
 ],'additionalProperties':False}
selector={'oneOf':[
 {'type':'object','required':['kind','value'],'properties':{'kind':{'const':'key'},'value':{'type':'string','minLength':1,'maxLength':512}},'additionalProperties':False},
 {'type':'object','required':['kind','value'],'properties':{'kind':{'const':'alias'},'value':slug},'additionalProperties':False}
]}
metric={'type':'object','required':['id','name','selector','value_type','units','origin'],'properties':{'id':slug,'name':nonempty,'selector':selector,'value_type':{'enum':['numeric_float','numeric_unsigned','character','text','log','binary']},'units':{'type':'string','maxLength':32},'origin':{'enum':['template_item','host_item','lld_prototype','discovered_item']},'uuid':{'type':'string','pattern':'^[0-9a-fA-F]{32}$'},'diagnostics':{'type':'array','uniqueItems':True,'items':{'enum':['missing_parent_template','unresolved_macros','lld_requires_instance','source_partial']}}},'additionalProperties':False}
# Source provenance: v0.5 `zabbix_version` is accepted ONLY as a legacy import field.
# Writers emit export_format_version for export inputs and source_server_version for API inputs.
source = {'type':'object','required':['kind'],'properties':{
 'kind':{'enum':['template_export','zabbix_api','manual']},
 'export_format_version':{'type':'string','pattern':version_pattern},
 'source_server_version':{'type':'string','pattern':version_pattern},
 'zabbix_version':{'type':'string','pattern':version_pattern,'deprecated':True},
 'template_name':{'type':'string','maxLength':200},
 'template_uuid':{'type':'string','pattern':'^[0-9a-fA-F]{32}$'}
},'allOf':[
 {'if':{'properties':{'kind':{'const':'template_export'}},'required':['kind']},'then':{'anyOf':[{'required':['export_format_version']},{'required':['zabbix_version']}]}},
 {'if':{'properties':{'kind':{'const':'zabbix_api'}},'required':['kind']},'then':{'anyOf':[{'required':['source_server_version']},{'required':['zabbix_version']}], 'not':{'required':['export_format_version']}}},
 {'if':{'properties':{'kind':{'const':'manual'}},'required':['kind']},'then':{'not':{'anyOf':[{'required':['export_format_version']},{'required':['source_server_version']}]}}},
 {'not':{'allOf':[{'required':['zabbix_version']},{'anyOf':[{'required':['export_format_version']},{'required':['source_server_version']}]}]}}
], 'additionalProperties':False}
metric_schema={'$schema':'https://json-schema.org/draft/2020-12/schema','$id':'https://zdashstitch.example/schemas/metrics-v1.schema.json','title':'ZDashstitch metric catalog v1','type':'object','required':['format','catalog_id','export_profile','source','diagnostics','metrics'],'properties':{
 'format':{'const':'zdashstitch.metrics/v1'},'catalog_id':slug,'export_profile':{'enum':['portable_with_public_keys','redacted_aliases']},
 'source':source,
 'diagnostics':{'type':'array','uniqueItems':True,'items':{'enum':['missing_parent_template','unresolved_macros','lld_requires_instance','source_partial']}},
 'metrics':{'type':'array','maxItems':20000,'items':metric}
 }, 'allOf':[
  {'if':{'properties':{'export_profile':{'const':'redacted_aliases'}},'required':['export_profile']},'then':{'properties':{'metrics':{'items':{'properties':{'selector':{'properties':{'kind':{'const':'alias'}}}}}}}}},
  {'if':{'properties':{'export_profile':{'const':'portable_with_public_keys'}},'required':['export_profile']},'then':{'properties':{'metrics':{'items':{'properties':{'selector':{'properties':{'kind':{'const':'key'}}}}}}}}}
 ],'additionalProperties':False}

# Local private bindings: shape generated here; semantic uniqueness + referential integrity
# require the validator because JSON Schema does not express property-wise uniqueness.
dec_id={'type':'string','pattern':'^[0-9]+$'}
local={'$schema':'https://json-schema.org/draft/2020-12/schema', 'title':'Private local bindings', 'type':'object',
 'required':['format','storage_scope','project_id','connection_id','deployment_id','roles'],
 'properties':{'format':{'const':'zdashstitch.local-bindings/v1'},'storage_scope':{'const':'local_only'},
 'project_id':slug,'connection_id':slug,'deployment_id':slug,
 'roles':{'type':'object','minProperties':1,'patternProperties':{'^[a-z][a-z0-9_-]{1,63}$':{'type':'array','minItems':1,'maxItems':256,'items':{'type':'object','required':['slot_id','hostid'],'properties':{'slot_id':slug,'hostid':dec_id},'additionalProperties':False}}},'additionalProperties':False},
 'metric_overrides':{'type':'array','items':{'type':'object','required':['catalog','metric','slot_id','itemid'],'properties':{'catalog':slug,'metric':slug,'slot_id':slug,'itemid':dec_id,'role':slug},'additionalProperties':False}}},
 'additionalProperties':False}

deployment={'$schema':'https://json-schema.org/draft/2020-12/schema','title':'Private deployment state — last VERIFIED state only', 'type':'object',
 'required':['format','storage_scope','project_id','connection_id','deployment_id','adapter_id','target_server_version','server_identity_fingerprint','status','last_verified_at','dashboards'],
 'properties':{'format':{'const':'zdashstitch.deployment-state/v1'},'storage_scope':{'const':'local_only'},
 'project_id':slug,'connection_id':slug,'deployment_id':slug,'adapter_id':slug,
 'target_server_version':{'type':'string','pattern':version_pattern},
 'server_identity_fingerprint':{'type':'string','pattern':'^[0-9a-f]{64}$'},
 'status':{'const':'verified'},
 'last_verified_at':{'type':'string','format':'date-time'},
 'last_operation_id':{'type':'string','minLength':1,'maxLength':128},
 'dashboards':{'type':'array','maxItems':16,'items':{'type':'object','required':['logical_id','dashboardid','fingerprint','pages'],
 'properties':{'logical_id':slug,'dashboardid':dec_id,'fingerprint':{'type':'string','pattern':'^[0-9a-f]{64}$'},
 'pages':{'type':'array','maxItems':32,'items':{'type':'object','required':['logical_id','dashboard_pageid','widgets'],
 'properties':{'logical_id':slug,'dashboard_pageid':dec_id,
 'widgets':{'type':'array','maxItems':256,'items':{'type':'object','required':['logical_id','widgetid'],
 'properties':{'logical_id':slug,'slot_id':slug,'widgetid':dec_id,'reference':{'type':'string','minLength':1,'maxLength':255}},'additionalProperties':False}}},'additionalProperties':False}}},
 'additionalProperties':False}}},
 'additionalProperties':False}

for filename,obj in [('project-v1.schema.json',project),('project-v2.schema.json',project_v2),('metrics-v1.schema.json',metric_schema),('local-bindings-v1.schema.json',local),('deployment-state-v1.schema.json',deployment)]:
 (root/'schemas'/filename).write_bytes((json.dumps(obj,ensure_ascii=False,indent=2)+'\n').encode('utf-8'))
 print(filename)
