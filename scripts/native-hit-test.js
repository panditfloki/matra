// The shared contract checks physical set_hot payloads, continuous group travel,
// narrow control-width lanes, and removal of hidden/disabled controls.
const {testNativeHandles} = require('./native-handle-test.js');
async function testHit() {
  return testNativeHandles(undefined,{counts:[6],insetCases:[[0,0,0,0]]});
}
module.exports={testHit};
if(require.main===module)testHit().then(result=>console.log(JSON.stringify(result,null,2))).catch(error=>{console.error(error);process.exitCode=1;});
