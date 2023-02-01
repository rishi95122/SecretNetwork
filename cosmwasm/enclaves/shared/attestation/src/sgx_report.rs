use std::collections::HashMap;
use std::convert::TryFrom;
use std::fmt::Formatter;

use log::*;

use lazy_static::lazy_static;

#[cfg(all(feature = "verify", feature = "sgx"))]
use sgx_tse::{rsgx_create_report, rsgx_self_report, rsgx_verify_report};

#[cfg(all(feature = "verify", feature = "sgx"))]
use sgx_types::{
    sgx_report_data_t, sgx_report_t, sgx_self_target, sgx_status_t, sgx_target_info_t, SgxResult,
};

use secret_attestation_token::{Error, NodeAuthPublicKey};

use enclave_ffi_types::NodeAuthResult;

/// A report generated by an enclave that contains measurement, identity and
/// other data related to enclave.
///
/// # Note
///
/// Do not confuse `SgxEnclaveReport` with `AttestationReport`.
/// `SgxEnclaveReport` is generated by SGX hardware and endorsed by Quoting
/// Enclave through local attestation. The endorsed `SgxEnclaveReport` is an
/// `SgxQuote`. The quote is then sent to some attestation service (IAS or
/// DCAP-based AS). The endorsed `SgxQuote` is an attestation report signed by
/// attestation service's private key, a.k.a., `EndorsedAttestationReport`.
pub struct SgxEnclaveReport {
    /// Security version number of host system's CPU
    pub cpu_svn: [u8; 16],
    /// Misc select bits for the target enclave. Reserved for future function
    /// extension.
    pub misc_select: u32,
    /// Attributes of the enclave, for example, whether the enclave is running
    /// in debug mode.
    pub attributes: SgxReportAttributes,
    /// Measurement value of the enclave. See
    /// [`EnclaveMeasurement`](../types/struct.EnclaveMeasurement.html)
    pub mr_enclave: [u8; 32],
    /// Measurement value of the public key that verified the enclave. See
    /// [`EnclaveMeasurement`](../types/struct.EnclaveMeasurement.html)
    pub mr_signer: [u8; 32],
    /// Product ID of the enclave
    pub isv_prod_id: u16,
    /// Security version number of the enclave
    pub isv_svn: u16,
    /// Set of data used for communication between enclave and target enclave
    pub report_data: [u8; 64],
}

pub struct SgxReportAttributes {
    pub flags: u64,
    pub xfrm: u64,
}

impl std::fmt::Debug for SgxReportAttributes {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "attributes-flags: {:?}", self.flags)?;
        writeln!(f, "attributes-xfrm: {:?}", self.xfrm)
    }
}

impl std::fmt::Debug for SgxEnclaveReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "cpu_svn: {:?}", self.cpu_svn)?;
        writeln!(f, "misc_select: {:?}", self.misc_select)?;
        writeln!(f, "attributes: {:?}", self.attributes)?;
        writeln!(f, "mr_enclave: {:?}", self.mr_enclave)?;
        writeln!(f, "mr_signer: {:?}", self.mr_signer)?;
        writeln!(f, "isv_prod_id: {}", self.isv_prod_id)?;
        writeln!(f, "isv_svn: {}", self.isv_svn)?;
        writeln!(f, "report_data: {:?}", &self.report_data.to_vec())
    }
}

impl SgxEnclaveReport {
    pub fn get_owner_key(&self) -> NodeAuthPublicKey {
        let mut pk = NodeAuthPublicKey::default();
        pk.copy_from_slice(&self.report_data[0..32]);
        pk
    }

    #[cfg(feature = "verify")]
    pub fn verify(&self) -> Result<(), NodeAuthResult> {
        let self_report = get_report()
            .map_err(|_| panic!("Failed to validate self report"))
            .unwrap();

        if self_report.body.mr_enclave.m != self.mr_enclave
            || self_report.body.attributes.flags != self.attributes.flags
            || self_report.body.attributes.xfrm != self.attributes.xfrm
        {
            error!("Report does not match current enclave!");
            return Err(NodeAuthResult::MrEnclaveMismatch);
        }

        Ok(())
    }

    /// Parse bytes of report into `SgxEnclaveReport`.
    pub fn parse_from<'a>(bytes: &'a [u8]) -> Result<Self, Error> {
        let mut pos: usize = 0;
        let mut take = |n: usize| -> Result<&'a [u8], Error> {
            if n > 0 && bytes.len() >= pos + n {
                let ret = &bytes[pos..pos + n];
                pos += n;
                Ok(ret)
            } else {
                error!("Enclave report parsing error - bad report size");
                Err(Error::ReportParseError)
            }
        };

        // Start parsing report by bytes following specifications. Don't
        // transmute directly, since there may cause endianness issue.
        // off 48, size 16
        let cpu_svn = <[u8; 16]>::try_from(take(16)?)?;

        // off 64, size 4
        let misc_select = u32::from_le_bytes(<[u8; 4]>::try_from(take(4)?)?);

        // off 68, size 28
        let _reserved = take(28)?;

        // off 96, size 16
        let flags = <[u8; 8]>::try_from(take(8)?)?;
        let xfrm = <[u8; 8]>::try_from(take(8)?)?;

        let attributes = SgxReportAttributes {
            flags: u64::from_le_bytes(flags),
            xfrm: u64::from_le_bytes(xfrm),
        };

        // off 112, size 32
        let mr_enclave = <[u8; 32]>::try_from(take(32)?)?;

        // off 144, size 32
        let _reserved = take(32)?;

        // off 176, size 32
        let mr_signer = <[u8; 32]>::try_from(take(32)?)?;

        // off 208, size 96
        let _reserved = take(96)?;

        // off 304, size 2
        let isv_prod_id = u16::from_le_bytes(<[u8; 2]>::try_from(take(2)?)?);

        // off 306, size 2
        let isv_svn = u16::from_le_bytes(<[u8; 2]>::try_from(take(2)?)?);

        // off 308, size 60
        let _reserved = take(60)?;

        // off 368, size 64
        let mut report_data = [0u8; 64];
        let _report_data = take(64)?;
        let mut _it = _report_data.iter();
        for i in report_data.iter_mut() {
            *i = *_it.next().ok_or(Error::ReportParseError)?;
        }

        if pos != bytes.len() {
            warn!("Enclave report parsing error.");
            return Err(Error::ReportParseError);
        };

        Ok(SgxEnclaveReport {
            cpu_svn,
            misc_select,
            attributes,
            mr_enclave,
            mr_signer,
            isv_prod_id,
            isv_svn,
            report_data,
        })
    }
}

#[cfg(feature = "verify")]
pub fn verify_report(report: sgx_report_t, target_info: sgx_target_info_t) -> SgxResult<()> {
    // Perform a check on qe_report to verify if the qe_report is valid
    match rsgx_verify_report(&report) {
        Ok(()) => trace!("rsgx_verify_report passed!"),
        Err(x) => {
            warn!("rsgx_verify_report failed with {:?}", x);
            return Err(x);
        }
    }

    // Check if the qe_report is produced on the same platform
    if target_info.mr_enclave.m != report.body.mr_enclave.m
        || target_info.attributes.flags != report.body.attributes.flags
        || target_info.attributes.xfrm != report.body.attributes.xfrm
    {
        error!("qe_report does not match current target_info!");
        return Err(sgx_status_t::SGX_ERROR_UNEXPECTED);
    }

    trace!("QE report check passed");
    Ok(())
}

#[cfg(not(feature = "production"))]
const WHITELISTED_ADVISORIES: &[&str] = &[
    "INTEL-SA-00334",
    "INTEL-SA-00219",
    "INTEL-SA-00615",
    "INTEL-SA-00657",
];

#[cfg(feature = "production")]
const WHITELISTED_ADVISORIES: &[&str] = &[
    "INTEL-SA-00334",
    "INTEL-SA-00219",
    "INTEL-SA-00615",
    "INTEL-SA-00657",
];

lazy_static! {
    static ref ADVISORY_DESC: HashMap<&'static str, &'static str> = [
        (
            "INTEL-SA-00161",
            "You must disable hyperthreading in the BIOS"
        ),
        (
            "INTEL-SA-00289",
            "You must disable overclocking/undervolting in the BIOS"
        ),
    ]
    .iter()
    .copied()
    .collect();
}

#[derive(Debug)]
pub struct AdvisoryIDs(pub Vec<String>);

// #[cfg(feature = "SGX_MODE_HW")]
impl AdvisoryIDs {
    pub(crate) fn vulnerable(&self) -> Vec<String> {
        let mut vulnerable: Vec<String> = vec![];
        for i in self.0.iter() {
            if !WHITELISTED_ADVISORIES.contains(&i.as_str()) {
                vulnerable.push(i.clone());
                if let Some(v) = ADVISORY_DESC.get(&i.as_str()) {
                    vulnerable.push((*v).to_string())
                }
            }
        }
        vulnerable
    }
}

#[cfg(feature = "verify")]
pub fn get_report() -> SgxResult<sgx_report_t> {
    let mut ti = sgx_target_info_t::default();
    let result = unsafe { sgx_self_target(&mut ti) };
    if result != sgx_status_t::SGX_SUCCESS {
        error!("Error while getting self target info {}", result);
        return Err(result);
    }

    let self_report = rsgx_create_report(&ti, &sgx_report_data_t::default())?;

    rsgx_verify_report(&self_report).map_err(|e| {
        error!("Error verifying self report: {}", e);
        e
    })?;

    Ok(self_report)
}

#[cfg(feature = "test")]
pub mod tests {
    use serde_json::json;
    use std::untrusted::fs::File;

    #[cfg(target_env = "sgx")]
    use std::sgxfs::Read;

    use super::*;

    fn tls_ra_cert_der_test() -> Vec<u8> {
        let mut cert = vec![];
        let mut f =
            File::open("../execute/src/registration/fixtures/attestation_cert_hw_invalid_test.der")
                .unwrap();
        f.read_to_end(&mut cert).unwrap();

        cert
    }

    fn tls_ra_cert_der_v3() -> Vec<u8> {
        let mut cert = vec![];
        let mut f = File::open("../execute/src/registration/fixtures/tls_ra_cert_v3.der").unwrap();
        f.read_to_end(&mut cert).unwrap();

        cert
    }

    fn tls_ra_cert_der_v4() -> Vec<u8> {
        let mut cert = vec![];
        let mut f =
            File::open("../execute/src/registration/fixtures/attestation_cert_out_of_date.der")
                .unwrap();
        f.read_to_end(&mut cert).unwrap();

        cert
    }

    fn _test_aes_encrypttls_ra_cert_der_out_of_date() -> Vec<u8> {
        let mut cert = vec![];
        let mut f = File::open(
            "../execute/src/registration/fixtures/attestation_cert_sw_config_needed.der",
        )
        .unwrap();
        f.read_to_end(&mut cert).unwrap();

        cert
    }

    fn _ias_root_ca_cert_der() -> Vec<u8> {
        let mut cert = vec![];
        let mut f =
            File::open("../execute/src/registration/fixtures/ias_root_ca_cert.der").unwrap();
        f.read_to_end(&mut cert).unwrap();

        cert
    }

    fn attesation_report() -> Value {
        let report = json!({
            "version": 3,
            "timestamp": "2020-02-11T22:25:59.682915",
            "platformInfoBlob": "1502006504000900000D0D02040180030000000000000000000\
                                 A00000B000000020000000000000B2FE0AE0F7FD4D552BF7EF4\
                                 C938D44E349F1BD0E76F041362DC52B43B7B25994978D792137\
                                 90362F6DAE91797ACF5BD5072E45F9A60795D1FFB10140421D8\
                                 691FFD",
            "isvEnclaveQuoteStatus": "GROUP_OUT_OF_DATE",
            "isvEnclaveQuoteBody": "AgABAC8LAAAKAAkAAAAAAK1zRQOIpndiP4IhlnW2AkwAAAAA\
                                    AAAAAAAAAAAAAAAABQ4CBf+AAAAAAAAAAAAAAAAAAAAAAAAA\
                                    AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABwAAAAAAAAAHAAAA\
                                    AAAAADMKqRCjd2eA4gAmrj2sB68OWpMfhPH4MH27hZAvWGlT\
                                    AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACD1xnn\
                                    ferKFHD2uvYqTXdDA8iZ22kCD5xw7h38CMfOngAAAAAAAAAA\
                                    AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\
                                    AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\
                                    AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\
                                    AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\
                                    AAAAAAAAAADYIY9k0MVmCdIDUuFLf/2bGIHAfPjO9nvC7fgz\
                                    rQedeA3WW4dFeI6oe+RCLdV3XYD1n6lEZjITOzPPLWDxulGz",
            "id": "53530608302195762335736519878284384788",
            "epidPseudonym": "NRksaQej8R/SyyHpZXzQGNBXqfrzPy5KCxcmJrEjupXrq3xrm2y2+J\
                              p0IBVtcW15MCekYs9K3UH82fPyj6F5ciJoMsgEMEIvRR+csX9uyd54\
                              p+m+/RVyuGYhWbhUcpJigdI5Q3x04GG/A7EP10j/zypwqhYLQh0qN1\
                              ykYt1N1P0="
        });

        report
    }

    pub fn test_sgx_quote_parse_from() {
        let attn_report = attesation_report();
        let sgx_quote_body_encoded = attn_report["isvEnclaveQuoteBody"].as_str().unwrap();
        let quote_raw = base64::decode(&sgx_quote_body_encoded.as_bytes()).unwrap();
        let sgx_quote = SgxQuote::parse_from(quote_raw.as_slice()).unwrap();

        assert_eq!(
            sgx_quote.version,
            crate::registration::attestation::sgx::sgx_quote::SgxQuoteVersion::V2(
                crate::registration::attestation::sgx::sgx_quote::SgxEpidQuoteSigType::Linkable
            )
        );
        assert_eq!(sgx_quote.gid, 2863);
        assert_eq!(sgx_quote.isv_svn_qe, 10);
        assert_eq!(sgx_quote.isv_svn_pce, 9);
        assert_eq!(
            sgx_quote.qe_vendor_id,
            Uuid::parse_str("00000000-ad73-4503-88a6-77623f822196").unwrap()
        );
        assert_eq!(
            sgx_quote.user_data,
            [117, 182, 2, 76, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );

        let isv_enclave_report = sgx_quote.isv_enclave_report;
        assert_eq!(
            isv_enclave_report.cpu_svn,
            [5, 14, 2, 5, 255, 128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(isv_enclave_report.misc_select, 0);
        assert_eq!(
            isv_enclave_report.attributes,
            [7, 0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            isv_enclave_report.mr_enclave,
            [
                51, 10, 169, 16, 163, 119, 103, 128, 226, 0, 38, 174, 61, 172, 7, 175, 14, 90, 147,
                31, 132, 241, 248, 48, 125, 187, 133, 144, 47, 88, 105, 83
            ]
        );
        assert_eq!(
            isv_enclave_report.mr_signer,
            [
                131, 215, 25, 231, 125, 234, 202, 20, 112, 246, 186, 246, 42, 77, 119, 67, 3, 200,
                153, 219, 105, 2, 15, 156, 112, 238, 29, 252, 8, 199, 206, 158
            ]
        );
        assert_eq!(isv_enclave_report.isv_prod_id, 0);
        assert_eq!(isv_enclave_report.isv_svn, 0);
        assert_eq!(
            isv_enclave_report.report_data.to_vec(),
            [
                216, 33, 143, 100, 208, 197, 102, 9, 210, 3, 82, 225, 75, 127, 253, 155, 24, 129,
                192, 124, 248, 206, 246, 123, 194, 237, 248, 51, 173, 7, 157, 120, 13, 214, 91,
                135, 69, 120, 142, 168, 123, 228, 66, 45, 213, 119, 93, 128, 245, 159, 169, 68,
                102, 50, 19, 59, 51, 207, 45, 96, 241, 186, 81, 179
            ]
            .to_vec()
        );
    }

    pub fn test_attestation_report_from_cert() {
        let tls_ra_cert = tls_ra_cert_der_v4();
        let report = ValidatedAttestation::from_cert(&tls_ra_cert);
        assert!(report.is_ok());

        let report = report.unwrap();
        assert_eq!(report.sgx_quote_status, SgxQuoteStatus::GroupOutOfDate);
    }

    pub fn test_attestation_report_from_cert_invalid() {
        let tls_ra_cert = tls_ra_cert_der_v4();
        let report = ValidatedAttestation::from_cert(&tls_ra_cert);
        assert!(report.is_ok());

        let report = report.unwrap();
        assert_eq!(report.sgx_quote_status, SgxQuoteStatus::GroupOutOfDate);
    }

    pub fn test_attestation_report_from_cert_api_version_not_compatible() {
        let tls_ra_cert = tls_ra_cert_der_v3();
        let report = ValidatedAttestation::from_cert(&tls_ra_cert);
        assert!(report.is_err());
    }

    pub fn test_attestation_report_test() {
        let tls_ra_cert = tls_ra_cert_der_test();
        let report = ValidatedAttestation::from_cert(&tls_ra_cert);

        if report.is_err() {
            println!("err: {:?}", report)
        }

        assert!(report.is_ok());
    }
}
