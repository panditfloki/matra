// Packaged geometry at three scales. The fixture/account guard lives in the shared runner.
const {testNativeHandles} = require('./native-handle-test.js');
async function testCompact() {
  return testNativeHandles(undefined,{scales:[.8,1,1.25],counts:[6]});
}
module.exports={testCompact};
if(require.main===module)testCompact().then(result=>console.log(JSON.stringify(result,null,2))).catch(error=>{console.error(error);process.exitCode=1;});
