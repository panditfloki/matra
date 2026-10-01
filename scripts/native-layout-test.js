// Disposable CI fixtures only. Serialize settings/fixture restoration between suites.
// Physical OS mouse travel is separately tested by native-physical-hover-test.ps1.
(async()=>{
  for(const test of [require('./native-compact-test.js').testCompact,require('./native-handle-test.js').testNativeHandles,require('./native-hit-test.js').testHit]) {
    console.log(JSON.stringify(await test(),null,2));
  }
})().catch(error=>{console.error(error);process.exitCode=1;});
